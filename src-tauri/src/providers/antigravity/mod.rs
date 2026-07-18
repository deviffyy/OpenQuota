mod auth;
mod client;
mod discovery;
mod mapper;

use std::path::PathBuf;

use chrono::Utc;
use serde_json::{json, Value};
use thiserror::Error;

use crate::models::{
    MetricDefinition, MetricSection, ProviderDefinition, ProviderSnapshot, UsageHistory,
};

use self::{
    auth::{load_token, AccessTokenCache},
    client::{AntigravityClient, CloudOutcome, CloudUserAgent, RefreshOutcome},
    discovery::discover,
    mapper::{
        build_legacy_quotas, parse_cloud_models, parse_command_model_configs, parse_plan,
        parse_quota_buckets, parse_quota_summary, parse_user_status,
    },
};

const QUOTA_SUMMARY_PATH: &str = "/v1internal:retrieveUserQuotaSummary";
const FETCH_MODELS_PATH: &str = "/v1internal:fetchAvailableModels";
const LOAD_CODE_ASSIST_PATH: &str = "/v1internal:loadCodeAssist";
const RETRIEVE_QUOTA_PATH: &str = "/v1internal:retrieveUserQuota";

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "antigravity".into(),
        display_name: "Antigravity".into(),
        short_name: "A".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        links: vec![],
        metrics: vec![
            MetricDefinition::quota(
                "antigravity.geminiPro",
                "Session",
                "geminiPro",
                true,
                true,
                MetricSection::AlwaysVisible,
                true,
                "S",
            ),
            MetricDefinition::quota(
                "antigravity.geminiWeekly",
                "Weekly",
                "geminiWeekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "W",
            ),
            MetricDefinition::quota(
                "antigravity.claude",
                "Claude",
                "claude",
                true,
                true,
                MetricSection::OnDemand,
                false,
                "C",
            ),
            MetricDefinition::quota(
                "antigravity.claudeWeekly",
                "Claude Weekly",
                "claudeWeekly",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "CW",
            ),
        ],
    }
}

#[derive(Debug, Error)]
pub enum AntigravityError {
    #[error("Start Antigravity or run `agy` and try again.")]
    NotSignedIn,
    #[error("Antigravity sign-in expired. Open Antigravity or run `agy` to refresh.")]
    AuthExpired,
    #[error("Antigravity credentials could not be read from secure storage.")]
    CredentialStoreUnreadable,
    #[error("Antigravity credentials are invalid. Sign in again in Antigravity or `agy`.")]
    InvalidCredentialData,
    #[error("Antigravity usage is temporarily unavailable. Try again shortly.")]
    Unavailable,
}

pub struct AntigravityProvider {
    client: AntigravityClient,
    access_token_cache: AccessTokenCache,
}

impl AntigravityProvider {
    pub fn new(cache_path: PathBuf) -> Result<Self, AntigravityError> {
        Ok(Self {
            client: AntigravityClient::new()?,
            access_token_cache: AccessTokenCache::new(cache_path),
        })
    }

    fn refresh_inner(&self) -> Result<ProviderSnapshot, AntigravityError> {
        if let Some(server) = discover() {
            if let Some(summary) = self
                .client
                .call_language_server(&server, "RetrieveUserQuotaSummary")
            {
                if let Some(quotas) = parse_quota_summary(&summary) {
                    let plan = self
                        .client
                        .call_language_server(&server, "GetUserStatus")
                        .as_ref()
                        .and_then(parse_plan);
                    return Ok(snapshot(plan, quotas));
                }
            }

            if let Some(status) = self.client.call_language_server(&server, "GetUserStatus") {
                if let Some((plan, configs)) = parse_user_status(&status) {
                    let quotas = build_legacy_quotas(configs);
                    if !quotas.is_empty() {
                        return Ok(snapshot(plan, quotas));
                    }
                }
                if let Some(configs) = self
                    .client
                    .call_language_server(&server, "GetCommandModelConfigs")
                    .as_ref()
                    .and_then(parse_command_model_configs)
                {
                    let quotas = build_legacy_quotas(configs);
                    if !quotas.is_empty() {
                        return Ok(snapshot(None, quotas));
                    }
                }
            }
        }

        let keychain = match load_token()? {
            Some(token) => token,
            None => {
                self.access_token_cache.discard();
                return Err(AntigravityError::NotSignedIn);
            }
        };
        let now = Utc::now();
        let cached = self
            .access_token_cache
            .load(keychain.refresh_token.as_deref(), now);
        let (access_tokens, access_is_expired) = access_token_candidates(&keychain, cached, now);

        let mut saw_auth_failure = access_is_expired;
        let mut saw_unavailable = false;
        for access_token in &access_tokens {
            match self.fetch_remote(access_token) {
                Ok(snapshot) => return Ok(snapshot),
                Err(AntigravityError::AuthExpired) => saw_auth_failure = true,
                Err(AntigravityError::Unavailable) => saw_unavailable = true,
                Err(error) => return Err(error),
            }
        }

        let refresh_token = keychain
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty());
        if let Some(refresh_token) = refresh_token.filter(|_| {
            should_refresh_access_token(saw_auth_failure, !access_tokens.is_empty(), true)
        }) {
            match self.client.refresh_google_token(refresh_token) {
                RefreshOutcome::Refreshed {
                    access_token,
                    expires_in_seconds,
                } => {
                    crate::app_info!("auth:antigravity", "token refresh succeeded");
                    self.access_token_cache.store(
                        &access_token,
                        expires_in_seconds,
                        Some(refresh_token),
                        Utc::now(),
                    );
                    return match self.fetch_remote(&access_token) {
                        Err(AntigravityError::AuthExpired) => {
                            self.access_token_cache.discard();
                            Err(AntigravityError::AuthExpired)
                        }
                        result => result,
                    };
                }
                RefreshOutcome::AuthFailed => {
                    self.access_token_cache.discard();
                    return Err(AntigravityError::AuthExpired);
                }
                RefreshOutcome::Unavailable => return Err(AntigravityError::Unavailable),
            }
        }
        Err(credential_failure(
            saw_auth_failure,
            saw_unavailable,
            !access_tokens.is_empty(),
        ))
    }

    fn fetch_remote(&self, token: &str) -> Result<ProviderSnapshot, AntigravityError> {
        match self.client.cloud_code(
            QUOTA_SUMMARY_PATH,
            token,
            json!({}),
            CloudUserAgent::Antigravity,
        ) {
            CloudOutcome::Ok(value) => {
                if let Some(quotas) = parse_quota_summary(&value) {
                    return Ok(snapshot(self.load_remote_plan(token), quotas));
                }
            }
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => {}
        }

        match self.client.cloud_code(
            FETCH_MODELS_PATH,
            token,
            json!({}),
            CloudUserAgent::Antigravity,
        ) {
            CloudOutcome::Ok(value) => {
                let quotas = build_legacy_quotas(parse_cloud_models(&value));
                if !quotas.is_empty() {
                    return Ok(snapshot(self.load_remote_plan(token), quotas));
                }
            }
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => {}
        }

        let (plan, project) = match self.client.cloud_code(
            LOAD_CODE_ASSIST_PATH,
            token,
            json!({}),
            CloudUserAgent::Agy,
        ) {
            CloudOutcome::Ok(value) => (
                remote_plan(&value),
                value
                    .get("cloudaicompanionProject")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned),
            ),
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => (None, None),
        };

        let body = project
            .as_ref()
            .map(|project| json!({"project": project}))
            .unwrap_or_else(|| json!({}));
        let mut quota =
            self.client
                .cloud_code(RETRIEVE_QUOTA_PATH, token, body, CloudUserAgent::Agy);
        if matches!(quota, CloudOutcome::Unavailable) && project.is_some() {
            quota =
                self.client
                    .cloud_code(RETRIEVE_QUOTA_PATH, token, json!({}), CloudUserAgent::Agy);
        }
        match quota {
            CloudOutcome::Ok(value) => {
                let quotas = build_legacy_quotas(parse_quota_buckets(&value));
                if !quotas.is_empty() {
                    return Ok(snapshot(plan, quotas));
                }
            }
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => {}
        }
        Err(AntigravityError::Unavailable)
    }

    fn load_remote_plan(&self, token: &str) -> Option<String> {
        match self
            .client
            .cloud_code(LOAD_CODE_ASSIST_PATH, token, json!({}), CloudUserAgent::Agy)
        {
            CloudOutcome::Ok(value) => remote_plan(&value),
            _ => None,
        }
    }
}

fn access_token_candidates(
    source: &auth::AntigravityToken,
    cached: Option<String>,
    now: chrono::DateTime<Utc>,
) -> (Vec<String>, bool) {
    let access_is_expired = source.access_token.as_ref().is_some_and(|_| {
        source
            .expiry
            .is_some_and(|expiry| expiry <= now + chrono::Duration::seconds(60))
    });
    let mut candidates = Vec::new();
    if !access_is_expired {
        if let Some(access_token) = source
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            candidates.push(access_token.to_owned());
        }
    }
    if let Some(cached) = cached.filter(|token| !token.trim().is_empty()) {
        if !candidates.iter().any(|token| token == &cached) {
            candidates.push(cached);
        }
    }
    (candidates, access_is_expired)
}

fn should_refresh_access_token(
    saw_auth_failure: bool,
    has_access_candidate: bool,
    has_refresh_token: bool,
) -> bool {
    has_refresh_token && (saw_auth_failure || !has_access_candidate)
}

fn credential_failure(
    saw_auth_failure: bool,
    saw_unavailable: bool,
    tried_access_token: bool,
) -> AntigravityError {
    if saw_auth_failure {
        AntigravityError::AuthExpired
    } else if saw_unavailable || tried_access_token {
        AntigravityError::Unavailable
    } else {
        AntigravityError::NotSignedIn
    }
}

fn remote_plan(value: &Value) -> Option<String> {
    let raw = value
        .pointer("/paidTier/name")
        .or_else(|| value.pointer("/currentTier/name"))
        .and_then(Value::as_str)?;
    for tier in ["Ultra", "Pro", "Free"] {
        if raw
            .to_ascii_lowercase()
            .contains(&tier.to_ascii_lowercase())
        {
            return Some(tier.into());
        }
    }
    Some(raw.into())
}

fn snapshot(plan: Option<String>, quotas: Vec<crate::models::QuotaWindow>) -> ProviderSnapshot {
    ProviderSnapshot {
        provider_id: "antigravity".into(),
        plan,
        quotas,
        value_metrics: Vec::new(),
        status_metrics: Vec::new(),
        notices: Vec::new(),
        usage: UsageHistory::default(),
        warnings: Vec::new(),
        refreshed_at: Utc::now(),
    }
}

fn has_refresh_source(
    has_credentials: bool,
    local_server_available: impl FnOnce() -> bool,
) -> bool {
    has_credentials || local_server_available()
}

impl crate::providers::UsageProvider for AntigravityProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        has_refresh_source(auth::has_local_credentials(), || discover().is_some())
    }

    fn refresh(&self) -> Result<ProviderSnapshot, crate::providers::ProviderError> {
        self.refresh_inner().map_err(|error| {
            use crate::models::ProviderErrorKind as Kind;

            let kind = match error {
                AntigravityError::NotSignedIn
                | AntigravityError::AuthExpired
                | AntigravityError::InvalidCredentialData => Kind::Authentication,
                AntigravityError::CredentialStoreUnreadable => Kind::CredentialStorage,
                AntigravityError::Unavailable => Kind::Network,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use chrono::{Duration, TimeZone, Utc};

    use super::{
        access_token_candidates, auth::AntigravityToken, credential_failure, has_refresh_source,
        should_refresh_access_token, AntigravityError,
    };

    #[test]
    fn keychain_access_token_precedes_a_distinct_cached_token() {
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();
        let source = AntigravityToken {
            access_token: Some("keychain-access".into()),
            refresh_token: Some("refresh".into()),
            expiry: Some(now + Duration::hours(1)),
        };

        let (candidates, expired) =
            access_token_candidates(&source, Some("cached-access".into()), now);
        assert_eq!(candidates, ["keychain-access", "cached-access"]);
        assert!(!expired);

        let (deduplicated, _) =
            access_token_candidates(&source, Some("keychain-access".into()), now);
        assert_eq!(deduplicated, ["keychain-access"]);
    }

    #[test]
    fn expiring_keychain_tokens_are_skipped_but_count_as_auth_evidence() {
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();
        let source = AntigravityToken {
            access_token: Some("expiring".into()),
            refresh_token: Some("refresh".into()),
            expiry: Some(now + Duration::seconds(30)),
        };

        let (candidates, expired) = access_token_candidates(&source, None, now);
        assert!(candidates.is_empty());
        assert!(expired);
    }

    #[test]
    fn refresh_and_terminal_error_decisions_preserve_auth_network_distinction() {
        assert!(!should_refresh_access_token(false, true, true));
        assert!(should_refresh_access_token(true, true, true));
        assert!(should_refresh_access_token(false, false, true));
        assert!(!should_refresh_access_token(true, true, false));

        assert!(matches!(
            credential_failure(true, false, true),
            AntigravityError::AuthExpired
        ));
        assert!(matches!(
            credential_failure(false, true, true),
            AntigravityError::Unavailable
        ));
        assert!(matches!(
            credential_failure(false, false, false),
            AntigravityError::NotSignedIn
        ));
    }

    #[test]
    fn running_language_server_is_a_refresh_source_without_credentials() {
        assert!(has_refresh_source(false, || true));
        assert!(!has_refresh_source(false, || false));
    }

    #[test]
    fn credentials_skip_language_server_discovery() {
        let discovery_called = Cell::new(false);

        assert!(has_refresh_source(true, || {
            discovery_called.set(true);
            false
        }));
        assert!(!discovery_called.get());
    }
}
