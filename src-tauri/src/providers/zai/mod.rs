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
    auth::ZaiAuthStore,
    client::{ZaiClient, ZaiResponse},
    mapper::{is_no_coding_plan, map_usage},
};

use super::{ProviderError, UsageProvider};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "zai".into(),
        display_name: "Z.ai".into(),
        short_name: "Z".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        links: vec![
            ProviderLink::new(
                "Dashboard",
                "https://z.ai/manage-apikey/coding-plan/personal/my-plan",
            ),
            ProviderLink::new("API Keys", "https://z.ai/manage-apikey/apikey-list"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "zai.session",
                "Session",
                "session",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "S",
            ),
            MetricDefinition::quota(
                "zai.weekly",
                "Weekly",
                "weekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "W",
            ),
            MetricDefinition::quota(
                "zai.webSearches",
                "Web Searches",
                "webSearches",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "Search",
            ),
        ],
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum ZaiError {
    #[error(
        "Add a Z.ai API key in Customize, set ZAI_API_KEY, or configure ~/.config/openquota/zai.json."
    )]
    MissingKey,
    #[error("The Z.ai API key is invalid. Check it at z.ai/manage-apikey/apikey-list.")]
    InvalidKey,
    #[error("Could not reach Z.ai. Check your internet connection.")]
    ConnectionFailed,
    #[error("Z.ai usage data is temporarily unavailable.")]
    InvalidResponse,
    #[error("Z.ai request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("No active GLM Coding Plan. Subscribe at z.ai/subscribe to view usage.")]
    NoCodingPlan,
    #[error("The Z.ai API key could not be read or updated.")]
    CredentialStorage,
}

impl From<ZaiError> for ProviderError {
    fn from(error: ZaiError) -> Self {
        let kind = match error {
            ZaiError::MissingKey | ZaiError::InvalidKey => ProviderErrorKind::Authentication,
            ZaiError::ConnectionFailed => ProviderErrorKind::Network,
            ZaiError::RequestFailed(429) => ProviderErrorKind::RateLimited,
            ZaiError::RequestFailed(401 | 403) => ProviderErrorKind::Authentication,
            ZaiError::NoCodingPlan => ProviderErrorKind::Permission,
            ZaiError::RequestFailed(_) | ZaiError::InvalidResponse => {
                ProviderErrorKind::InvalidResponse
            }
            ZaiError::CredentialStorage => ProviderErrorKind::CredentialStorage,
        };
        ProviderError::new(kind, error.to_string())
    }
}

pub struct ZaiProvider {
    auth: ZaiAuthStore,
    client: Arc<ZaiClient>,
}

impl ZaiProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            auth: ZaiAuthStore::new(),
            client: Arc::new(ZaiClient::new().map_err(ProviderError::from)?),
        })
    }

    #[cfg(test)]
    fn with_dependencies(auth: ZaiAuthStore, client: ZaiClient) -> Self {
        Self {
            auth,
            client: Arc::new(client),
        }
    }

    fn refresh_snapshot(&self, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
        let quota = required_response(self.client.fetch_quota(api_key))?;
        if is_no_coding_plan(&quota.body) {
            return Err(ZaiError::NoCodingPlan.into());
        }
        let subscription = self
            .client
            .fetch_subscription(api_key)
            .ok()
            .filter(|response| response.status.is_success());
        let mapped = map_usage(
            &quota.body,
            subscription.as_ref().map(|response| &response.body),
        )?;
        Ok(ProviderSnapshot {
            provider_id: "zai".into(),
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

impl UsageProvider for ZaiProvider {
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
            .ok_or_else(|| ProviderError::from(ZaiError::MissingKey))?;
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

fn required_response(response: Result<ZaiResponse, ZaiError>) -> Result<ZaiResponse, ZaiError> {
    let response = response?;
    if matches!(
        response.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err(ZaiError::InvalidKey);
    }
    if !response.status.is_success() {
        return Err(ZaiError::RequestFailed(response.status.as_u16()));
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
        models::{ApiKeyStatus, MetricSection, ProviderErrorKind, QuotaFormat},
        providers::{
            api_key::{ApiKeyStore, EnvironmentReader, SecretBackend, SecretBytes},
            test_http, UsageProvider,
        },
    };

    use super::{auth::ZaiAuthStore, client::ZaiClient, definition, ZaiProvider};

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

    fn auth(key: Option<&str>) -> ZaiAuthStore {
        ZaiAuthStore::with_store(ApiKeyStore::with_backends(
            "zai",
            "ZAI_API_KEY",
            Arc::new(MemorySecrets::default()),
            Arc::new(Environment(
                key.map(|value| HashMap::from([("ZAI_API_KEY".into(), value.into())]))
                    .unwrap_or_default(),
            )),
        ))
    }

    fn provider(
        key: Option<&str>,
        quota_status: u16,
        quota_body: &str,
        subscription_status: u16,
        subscription_body: &str,
    ) -> ZaiProvider {
        let quota_url = test_http::serve_once(quota_status, &[], quota_body);
        let subscription_url = test_http::serve_once(subscription_status, &[], subscription_body);
        ZaiProvider::with_dependencies(
            auth(key),
            ZaiClient::for_test(&subscription_url, &quota_url, Duration::from_secs(1)),
        )
    }

    #[test]
    fn refresh_maps_required_quota_and_optional_subscription() {
        let snapshot = provider(
            Some("secret"),
            200,
            include_str!("fixtures/quota.json"),
            200,
            include_str!("fixtures/subscription.json"),
        )
        .refresh()
        .unwrap();

        assert_eq!(snapshot.plan.as_deref(), Some("GLM Coding Pro"));
        assert_eq!(
            snapshot
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly", "webSearches"]
        );
        assert_eq!(snapshot.quotas[2].format, QuotaFormat::Count);
        assert_eq!(snapshot.quotas[2].unit.as_deref(), Some("searches"));
        assert!(snapshot.status_metrics.is_empty());
        assert!(snapshot.warnings.is_empty());
    }

    #[test]
    fn subscription_failure_does_not_blank_required_quota() {
        let snapshot = provider(
            Some("secret"),
            200,
            include_str!("fixtures/quota.json"),
            503,
            "{}",
        )
        .refresh()
        .unwrap();

        assert_eq!(snapshot.plan, None);
        assert_eq!(snapshot.quotas.len(), 3);
    }

    #[test]
    fn missing_invalid_and_rate_limited_keys_are_distinct() {
        let missing = provider(None, 200, "{}", 200, "{}").refresh().unwrap_err();
        assert_eq!(missing.kind(), ProviderErrorKind::Authentication);
        assert!(missing.to_string().contains("Add a Z.ai API key"));

        for status in [401, 403] {
            let invalid = provider(Some("bad-key"), status, "{}", 200, "{}")
                .refresh()
                .unwrap_err();
            assert_eq!(invalid.kind(), ProviderErrorKind::Authentication);
            assert!(invalid.to_string().contains("invalid"));
            assert!(!invalid.to_string().contains("bad-key"));
        }

        let rate_limited = provider(Some("secret"), 429, "{}", 200, "{}")
            .refresh()
            .unwrap_err();
        assert_eq!(rate_limited.kind(), ProviderErrorKind::RateLimited);
    }

    #[test]
    fn no_coding_plan_and_malformed_payloads_are_typed() {
        let no_plan = provider(
            Some("secret"),
            200,
            r#"{"code":500,"msg":"Current user has no coding plan","success":false}"#,
            200,
            include_str!("fixtures/subscription.json"),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(no_plan.kind(), ProviderErrorKind::Permission);
        assert!(no_plan.to_string().contains("GLM Coding Plan"));

        let malformed = provider(
            Some("secret"),
            200,
            r#"{"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"number":5}]}}"#,
            200,
            include_str!("fixtures/subscription.json"),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(malformed.kind(), ProviderErrorKind::InvalidResponse);
    }

    #[test]
    fn transport_and_timeout_errors_do_not_expose_the_key() {
        let subscription_url = test_http::serve_once(200, &[], "{}");
        let provider = ZaiProvider::with_dependencies(
            auth(Some("super-secret-key")),
            ZaiClient::for_test(
                &subscription_url,
                "http://127.0.0.1:1",
                Duration::from_millis(100),
            ),
        );
        let error = provider.refresh().unwrap_err();
        assert_eq!(error.kind(), ProviderErrorKind::Network);
        assert!(!error.to_string().contains("super-secret-key"));

        let delayed = test_http::serve_once_after(
            test_http::TIMEOUT_TEST_RESPONSE_DELAY,
            200,
            &[],
            include_str!("fixtures/quota.json"),
        );
        let subscription_url = test_http::serve_once(200, &[], "{}");
        let timeout = ZaiProvider::with_dependencies(
            auth(Some("another-secret")),
            ZaiClient::for_test(
                &subscription_url,
                &delayed,
                test_http::TIMEOUT_TEST_CLIENT_LIMIT,
            ),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(timeout.kind(), ProviderErrorKind::Network);
        assert!(!timeout.to_string().contains("another-secret"));
    }

    #[test]
    fn api_key_capability_uses_the_secure_vault_and_falls_back_to_environment() {
        let provider = provider(
            Some("environment-key"),
            200,
            include_str!("fixtures/quota.json"),
            200,
            include_str!("fixtures/subscription.json"),
        );
        assert_eq!(
            provider.api_key_status().unwrap().unwrap(),
            ApiKeyStatus::FromEnvironment
        );

        provider.save_api_key("saved-key").unwrap();
        assert_eq!(
            provider.api_key_status().unwrap().unwrap(),
            ApiKeyStatus::OverrideActive
        );

        provider.delete_api_key().unwrap();
        assert_eq!(
            provider.api_key_status().unwrap().unwrap(),
            ApiKeyStatus::FromEnvironment
        );
    }

    #[test]
    fn definition_exposes_expected_links_and_default_metric_layout() {
        let definition = definition();
        assert_eq!(definition.id, "zai");
        assert_eq!(definition.display_name, "Z.ai");
        assert_eq!(
            definition
                .links
                .iter()
                .map(|link| link.label.as_str())
                .collect::<Vec<_>>(),
            ["Dashboard", "API Keys"]
        );

        let metric = |id: &str| {
            definition
                .metrics
                .iter()
                .find(|metric| metric.id == id)
                .unwrap()
        };
        assert_eq!(
            metric("zai.session").default_section,
            MetricSection::AlwaysVisible
        );
        assert!(metric("zai.session").default_pinned);
        assert!(!metric("zai.session").source.session_window());
        assert_eq!(
            metric("zai.weekly").default_section,
            MetricSection::AlwaysVisible
        );
        assert!(metric("zai.weekly").default_pinned);
        assert_eq!(
            metric("zai.webSearches").default_section,
            MetricSection::OnDemand
        );
        assert!(!metric("zai.webSearches").default_pinned);
    }
}
