mod client;
mod mapper;

use std::sync::Arc;

use chrono::Utc;
use reqwest::StatusCode;
use thiserror::Error;

use crate::{
    models::{
        ApiKeyStatus, MetricDefinition, MetricSection, ProviderDefinition, ProviderErrorKind,
        ProviderLink, ProviderSnapshot, UsageHistory,
    },
    providers::api_key::ApiKeyStore,
};

use self::{
    client::{EndpointResponse, OpenRouterClient},
    mapper::{data_object, map_credits, map_key},
};

use super::{ProviderError, UsageProvider};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "openrouter".into(),
        display_name: "OpenRouter".into(),
        short_name: "OR".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        links: vec![
            ProviderLink::new("Activity", "https://openrouter.ai/activity"),
            ProviderLink::new("Credits", "https://openrouter.ai/settings/credits"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "openrouter.credits",
                "Credits",
                "credits",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "C",
            ),
            MetricDefinition::value(
                "openrouter.balance",
                "Balance",
                "balance",
                true,
                MetricSection::AlwaysVisible,
                false,
                "B",
                None,
            ),
            MetricDefinition::value(
                "openrouter.today",
                "Today",
                "today",
                true,
                MetricSection::OnDemand,
                false,
                "T",
                None,
            ),
            MetricDefinition::value(
                "openrouter.week",
                "This Week",
                "week",
                true,
                MetricSection::OnDemand,
                false,
                "W",
                None,
            ),
            MetricDefinition::value(
                "openrouter.month",
                "This Month",
                "month",
                true,
                MetricSection::OnDemand,
                false,
                "M",
                None,
            ),
            MetricDefinition::quota(
                "openrouter.keyLimit",
                "Key Limit",
                "keyLimit",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "K",
            ),
        ],
    }
}

#[derive(Debug, Error)]
enum OpenRouterError {
    #[error("Add an OpenRouter API key in Customize to view usage.")]
    MissingKey,
    #[error("The OpenRouter API key is invalid. Check it at openrouter.ai/keys.")]
    InvalidKey,
    #[error("Could not reach OpenRouter. Check your internet connection.")]
    ConnectionFailed,
    #[error("OpenRouter usage data is temporarily unavailable.")]
    InvalidResponse,
    #[error("OpenRouter request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("The OpenRouter API key could not be read or updated.")]
    CredentialStorage,
}

impl From<OpenRouterError> for ProviderError {
    fn from(error: OpenRouterError) -> Self {
        let kind = match error {
            OpenRouterError::MissingKey | OpenRouterError::InvalidKey => {
                ProviderErrorKind::Authentication
            }
            OpenRouterError::ConnectionFailed => ProviderErrorKind::Network,
            OpenRouterError::RequestFailed(429) => ProviderErrorKind::RateLimited,
            OpenRouterError::RequestFailed(401 | 403) => ProviderErrorKind::Authentication,
            OpenRouterError::RequestFailed(_) | OpenRouterError::InvalidResponse => {
                ProviderErrorKind::InvalidResponse
            }
            OpenRouterError::CredentialStorage => ProviderErrorKind::CredentialStorage,
        };
        ProviderError::new(kind, error.to_string())
    }
}

pub struct OpenRouterProvider {
    auth: ApiKeyStore,
    client: Arc<OpenRouterClient>,
}

impl OpenRouterProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            auth: ApiKeyStore::new("openrouter", "OPENROUTER_API_KEY"),
            client: Arc::new(OpenRouterClient::new().map_err(ProviderError::from)?),
        })
    }

    #[cfg(test)]
    fn with_dependencies(auth: ApiKeyStore, client: OpenRouterClient) -> Self {
        Self {
            auth,
            client: Arc::new(client),
        }
    }

    fn refresh_snapshot(&self, api_key: &str) -> Result<ProviderSnapshot, ProviderError> {
        let (credits, key) = std::thread::scope(|scope| {
            let credits = scope.spawn(|| self.client.fetch_credits(api_key));
            let key = scope.spawn(|| self.client.fetch_key(api_key));
            (
                credits
                    .join()
                    .unwrap_or(Err(OpenRouterError::ConnectionFailed)),
                key.join().unwrap_or(Err(OpenRouterError::ConnectionFailed)),
            )
        });
        let credits = classify(credits);
        let key = classify(key);
        let mut quotas = Vec::new();
        let mut values = Vec::new();
        let mut plan = None;

        if let EndpointOutcome::Success(data) = &credits {
            let mapped = map_credits(data);
            quotas.extend(mapped.quota);
            values.extend(mapped.balance);
        }
        if let EndpointOutcome::Success(data) = &key {
            let mapped = map_key(data);
            plan = mapped.plan;
            quotas.extend(mapped.quota);
            values.extend(mapped.values);
        }
        if !quotas.is_empty() || !values.is_empty() {
            return Ok(ProviderSnapshot {
                provider_id: "openrouter".into(),
                plan,
                quotas,
                value_metrics: values,
                notices: Vec::new(),
                usage: UsageHistory::default(),
                warnings: Vec::new(),
                refreshed_at: Utc::now(),
            });
        }
        if credits.is_auth_failure() && key.is_auth_failure() {
            return Err(OpenRouterError::InvalidKey.into());
        }
        Err(credits
            .error()
            .or_else(|| key.error())
            .unwrap_or(OpenRouterError::InvalidResponse)
            .into())
    }
}

impl UsageProvider for OpenRouterProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        self.auth.load().is_ok_and(|key| key.is_some())
    }

    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
        let api_key = self
            .auth
            .load()
            .map_err(|_| ProviderError::from(OpenRouterError::CredentialStorage))?
            .ok_or_else(|| ProviderError::from(OpenRouterError::MissingKey))?;
        self.refresh_snapshot(api_key.as_str())
    }

    fn api_key_status(&self) -> Option<Result<ApiKeyStatus, ProviderError>> {
        Some(
            self.auth
                .status()
                .map_err(|_| ProviderError::from(OpenRouterError::CredentialStorage)),
        )
    }

    fn save_api_key(&self, value: &str) -> Result<(), ProviderError> {
        self.auth
            .save(value)
            .map_err(|_| ProviderError::from(OpenRouterError::CredentialStorage))
    }

    fn delete_api_key(&self) -> Result<(), ProviderError> {
        self.auth
            .delete()
            .map_err(|_| ProviderError::from(OpenRouterError::CredentialStorage))
    }
}

#[derive(Debug)]
enum EndpointOutcome {
    Success(serde_json::Map<String, serde_json::Value>),
    AuthFailure,
    Failed(OpenRouterError),
}

impl EndpointOutcome {
    fn is_auth_failure(&self) -> bool {
        matches!(self, Self::AuthFailure)
    }

    fn error(&self) -> Option<OpenRouterError> {
        match self {
            Self::Failed(OpenRouterError::ConnectionFailed) => {
                Some(OpenRouterError::ConnectionFailed)
            }
            Self::Failed(OpenRouterError::InvalidResponse) => {
                Some(OpenRouterError::InvalidResponse)
            }
            Self::Failed(OpenRouterError::RequestFailed(status)) => {
                Some(OpenRouterError::RequestFailed(*status))
            }
            Self::Success(_) | Self::AuthFailure | Self::Failed(_) => None,
        }
    }
}

fn classify(response: Result<EndpointResponse, OpenRouterError>) -> EndpointOutcome {
    let response = match response {
        Ok(response) => response,
        Err(error) => return EndpointOutcome::Failed(error),
    };
    if matches!(
        response.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return EndpointOutcome::AuthFailure;
    }
    if !response.status.is_success() {
        return EndpointOutcome::Failed(OpenRouterError::RequestFailed(response.status.as_u16()));
    }
    let Some(data) = data_object(&response.body) else {
        return EndpointOutcome::Failed(OpenRouterError::InvalidResponse);
    };
    EndpointOutcome::Success(data.clone())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use crate::{
        models::{ApiKeyStatus, ProviderErrorKind},
        providers::{
            api_key::{ApiKeyStore, EnvironmentReader, SecretBackend, SecretBytes},
            test_http, UsageProvider,
        },
    };

    use super::{client::OpenRouterClient, definition, OpenRouterProvider};

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
                .insert(account.into(), value.to_vec());
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

    fn auth(key: Option<&str>) -> ApiKeyStore {
        auth_from_environment(
            key.map(|value| [("OPENROUTER_API_KEY".to_owned(), value.to_owned())].into()),
        )
    }

    fn auth_from_environment(environment: Option<HashMap<String, String>>) -> ApiKeyStore {
        ApiKeyStore::with_backends(
            "openrouter",
            "OPENROUTER_API_KEY",
            Arc::new(MemorySecrets::default()),
            Arc::new(Environment(environment.unwrap_or_default())),
        )
    }

    fn provider(
        key: Option<&str>,
        credits_status: u16,
        credits_body: &str,
        key_status: u16,
        key_body: &str,
    ) -> OpenRouterProvider {
        let credits = test_http::serve_once(credits_status, &[], credits_body);
        let key_url = test_http::serve_once(key_status, &[], key_body);
        OpenRouterProvider::with_dependencies(
            auth(key),
            OpenRouterClient::for_test(&credits, &key_url, Duration::from_secs(1)),
        )
    }

    #[test]
    fn maps_both_endpoints_and_preserves_measured_zero() {
        let snapshot = provider(
            Some("secret"),
            200,
            include_str!("fixtures/credits.json"),
            200,
            include_str!("fixtures/key.json"),
        )
        .refresh()
        .unwrap();
        assert_eq!(snapshot.plan.as_deref(), Some("Pay as you go"));
        assert!(snapshot.quotas.iter().any(|quota| quota.id == "credits"));
        assert_eq!(
            snapshot
                .value_metrics
                .iter()
                .find(|metric| metric.id == "today")
                .unwrap()
                .values[0]
                .number,
            0.0
        );
    }

    #[test]
    fn one_gated_endpoint_does_not_blank_the_other() {
        let snapshot = provider(
            Some("secret"),
            403,
            "{}",
            200,
            r#"{"data":{"is_free_tier":false,"usage_daily":0.5}}"#,
        )
        .refresh()
        .unwrap();
        assert!(snapshot
            .value_metrics
            .iter()
            .any(|metric| metric.id == "today"));
        assert!(!snapshot
            .value_metrics
            .iter()
            .any(|metric| metric.id == "balance"));
    }

    #[test]
    fn both_auth_failures_are_invalid_but_missing_key_is_typed() {
        let error = provider(Some("bad"), 401, "{}", 403, "{}")
            .refresh()
            .unwrap_err();
        assert_eq!(error.kind(), ProviderErrorKind::Authentication);
        assert!(error.to_string().contains("invalid"));

        let missing = provider(None, 200, r#"{"data":{}}"#, 200, r#"{"data":{}}"#)
            .refresh()
            .unwrap_err();
        assert_eq!(missing.kind(), ProviderErrorKind::Authentication);
        assert!(missing.to_string().contains("Add an OpenRouter API key"));
    }

    #[test]
    fn api_key_capability_delegates_without_exposing_the_secret() {
        let provider = provider(
            Some("environment"),
            200,
            r#"{"data":{}}"#,
            200,
            r#"{"data":{}}"#,
        );
        assert_eq!(
            provider.api_key_status().unwrap().unwrap(),
            ApiKeyStatus::FromEnvironment
        );
        provider.save_api_key("saved").unwrap();
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
    fn default_layout_matches_the_reference_provider() {
        let definition = definition();
        let metric = |id: &str| {
            definition
                .metrics
                .iter()
                .find(|metric| metric.id == id)
                .unwrap()
        };

        assert_eq!(
            metric("openrouter.credits").default_section,
            crate::models::MetricSection::AlwaysVisible
        );
        assert!(metric("openrouter.credits").default_pinned);
        assert_eq!(
            metric("openrouter.balance").default_section,
            crate::models::MetricSection::AlwaysVisible
        );
        assert!(!metric("openrouter.balance").default_pinned);

        for id in [
            "openrouter.today",
            "openrouter.week",
            "openrouter.month",
            "openrouter.keyLimit",
        ] {
            assert_eq!(
                metric(id).default_section,
                crate::models::MetricSection::OnDemand
            );
            assert!(!metric(id).default_pinned);
        }
    }
}
