mod auth;
mod client;
mod mapper;

use std::sync::Arc;

use chrono::Utc;
use reqwest::StatusCode;
use thiserror::Error;

use crate::models::{
    ApiKeyStatus, MetricDefinition, MetricSection, ProviderDefinition, ProviderErrorKind,
    ProviderLink, ProviderSnapshot, UsageHistory,
};

use self::{
    auth::MiniMaxAuthStore,
    client::{EndpointResponse, MiniMaxClient},
    mapper::map_usage,
};

use super::{ProviderError, UsageProvider};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "minimax".into(),
        display_name: "MiniMax".into(),
        short_name: "M".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        links: vec![
            ProviderLink::new("Dashboard", "https://platform.minimax.io/console/plan"),
            ProviderLink::new("API Keys", "https://platform.minimax.io/console/access"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "minimax.session",
                "Session",
                "session",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "S",
            ),
            MetricDefinition::quota(
                "minimax.weekly",
                "Weekly",
                "weekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "W",
            ),
        ],
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum MiniMaxError {
    #[error("Add a MiniMax API key in Customize or set MINIMAX_API_KEY.")]
    MissingKey,
    #[error("The MiniMax API key is invalid. Check it at minimax.io.")]
    InvalidKey,
    #[error("Could not reach MiniMax. Check your internet connection.")]
    ConnectionFailed,
    #[error("MiniMax usage data is temporarily unavailable.")]
    InvalidResponse,
    #[error("MiniMax request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("No active MiniMax token plan. Subscribe at minimax.io to view usage.")]
    NoTokenPlan,
    #[error("The MiniMax API key could not be read or updated.")]
    CredentialStorage,
}

impl From<MiniMaxError> for ProviderError {
    fn from(error: MiniMaxError) -> Self {
        let kind = match error {
            MiniMaxError::MissingKey | MiniMaxError::InvalidKey => {
                ProviderErrorKind::Authentication
            }
            MiniMaxError::ConnectionFailed => ProviderErrorKind::Network,
            MiniMaxError::RequestFailed(429) => ProviderErrorKind::RateLimited,
            MiniMaxError::RequestFailed(401 | 403) => ProviderErrorKind::Authentication,
            MiniMaxError::NoTokenPlan => ProviderErrorKind::Permission,
            MiniMaxError::RequestFailed(_) | MiniMaxError::InvalidResponse => {
                ProviderErrorKind::InvalidResponse
            }
            MiniMaxError::CredentialStorage => ProviderErrorKind::CredentialStorage,
        };
        ProviderError::new(kind, error.to_string())
    }
}

pub struct MiniMaxProvider {
    auth: MiniMaxAuthStore,
    client: Arc<MiniMaxClient>,
}

impl MiniMaxProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            auth: MiniMaxAuthStore::new(),
            client: Arc::new(MiniMaxClient::new().map_err(ProviderError::from)?),
        })
    }

    #[cfg(test)]
    fn with_dependencies(auth: MiniMaxAuthStore, client: MiniMaxClient) -> Self {
        Self {
            auth,
            client: Arc::new(client),
        }
    }

    fn refresh_snapshot(&self, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
        let response = required_response(self.client.fetch(api_key))?;
        let mapped = map_usage(&response.body)?;
        Ok(ProviderSnapshot {
            provider_id: "minimax".into(),
            plan: mapped.plan,
            quotas: mapped.quotas,
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        })
    }
}

impl UsageProvider for MiniMaxProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        self.auth.has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
        let api_key = self
            .auth
            .load()
            .map_err(ProviderError::from)?
            .ok_or_else(|| ProviderError::from(MiniMaxError::MissingKey))?;
        self.refresh_snapshot(api_key.as_str())
    }

    fn api_key_status(&self) -> Option<Result<ApiKeyStatus, ProviderError>> {
        Some(self.auth.status().map_err(ProviderError::from))
    }

    fn save_api_key(&self, value: &str) -> Result<(), ProviderError> {
        self.auth.save(value).map_err(ProviderError::from)
    }

    fn delete_api_key(&self) -> Result<(), ProviderError> {
        self.auth.delete().map_err(ProviderError::from)
    }
}

fn required_response(
    response: Result<EndpointResponse, MiniMaxError>,
) -> Result<EndpointResponse, MiniMaxError> {
    let response = response?;
    if matches!(
        response.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err(MiniMaxError::InvalidKey);
    }
    if !response.status.is_success() {
        return Err(MiniMaxError::RequestFailed(response.status.as_u16()));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use crate::{
        models::ProviderErrorKind,
        providers::{api_key::*, test_http, UsageProvider},
    };

    use super::{auth::MiniMaxAuthStore, client::MiniMaxClient, definition, MiniMaxProvider};

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<String, Vec<u8>>>);

    impl SecretBackend for MemorySecrets {
        fn read(&self, account: &str) -> Result<Option<SecretBytes>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .map(SecretBytes::new))
        }
        fn write(&self, account: &str, value: &[u8]) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_owned(), value.to_vec());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    struct Environment(HashMap<String, String>);
    impl EnvironmentReader for Environment {
        fn value(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    fn auth(key: Option<&str>) -> MiniMaxAuthStore {
        MiniMaxAuthStore::with_store(ApiKeyStore::with_backends(
            "minimax",
            "MINIMAX_API_KEY",
            Arc::new(MemorySecrets::default()),
            Arc::new(Environment(
                key.map(|value| HashMap::from([("MINIMAX_API_KEY".into(), value.into())]))
                    .unwrap_or_default(),
            )),
        ))
    }

    const REMAINS_BODY: &str = r#"{"model_remains":[{
        "start_time":1786060800000,"end_time":1786078800000,"remains_time":2185461,
        "current_interval_total_count":0,"current_interval_usage_count":0,"model_name":"general",
        "current_weekly_total_count":0,"current_weekly_usage_count":0,
        "weekly_start_time":1785715200000,"weekly_end_time":1786320000000,"weekly_remains_time":243385461,
        "current_interval_status":2,"current_interval_remaining_percent":0,
        "current_weekly_status":3,"current_weekly_remaining_percent":100}],
        "base_resp":{"status_code":0,"status_msg":"success"}}"#;

    #[test]
    fn refresh_maps_weekly_and_interval() {
        let url = test_http::serve_once(200, &[], REMAINS_BODY);
        let provider = MiniMaxProvider::with_dependencies(
            auth(Some("secret")),
            MiniMaxClient::for_test(&url, Duration::from_secs(1)),
        );

        let snapshot = provider.refresh().unwrap();
        assert_eq!(snapshot.provider_id, "minimax");
        assert_eq!(snapshot.plan.as_deref(), Some("Token Plan"));
        assert_eq!(
            snapshot
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly"]
        );
        assert_eq!(snapshot.quotas[1].label, "Weekly (Unlimited)");
    }

    #[test]
    fn missing_invalid_and_rate_limited_keys_are_distinct() {
        let missing = MiniMaxProvider::with_dependencies(
            auth(None),
            MiniMaxClient::for_test(
                &test_http::serve_once(200, &[], REMAINS_BODY),
                Duration::from_secs(1),
            ),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(missing.kind(), ProviderErrorKind::Authentication);

        for status in [401, 403] {
            let invalid = MiniMaxProvider::with_dependencies(
                auth(Some("bad-key")),
                MiniMaxClient::for_test(
                    &test_http::serve_once(status, &[], "{}"),
                    Duration::from_secs(1),
                ),
            )
            .refresh()
            .unwrap_err();
            assert_eq!(invalid.kind(), ProviderErrorKind::Authentication);
            assert!(!invalid.to_string().contains("bad-key"));
        }

        let rate_limited = MiniMaxProvider::with_dependencies(
            auth(Some("secret")),
            MiniMaxClient::for_test(
                &test_http::serve_once(429, &[], "{}"),
                Duration::from_secs(1),
            ),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(rate_limited.kind(), ProviderErrorKind::RateLimited);
    }

    #[test]
    fn no_token_plan_is_a_permission_error() {
        let url = test_http::serve_once(
            200,
            &[],
            r#"{"base_resp":{"status_code":1001,"status_msg":"user has no token plan"}}"#,
        );
        let provider = MiniMaxProvider::with_dependencies(
            auth(Some("secret")),
            MiniMaxClient::for_test(&url, Duration::from_secs(1)),
        );
        let error = provider.refresh().unwrap_err();
        assert_eq!(error.kind(), ProviderErrorKind::Permission);
    }

    #[test]
    fn definition_exposes_expected_identity_and_metrics() {
        let definition = definition();
        assert_eq!(definition.id, "minimax");
        assert_eq!(definition.display_name, "MiniMax");
        assert_eq!(
            definition
                .links
                .iter()
                .map(|link| link.label.as_str())
                .collect::<Vec<_>>(),
            ["Dashboard", "API Keys"]
        );
    }
}
