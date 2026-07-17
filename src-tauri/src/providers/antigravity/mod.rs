mod auth;
mod client;
mod discovery;
mod mapper;

use std::sync::Mutex;

use chrono::Utc;
use serde_json::{json, Value};
use thiserror::Error;

use crate::models::{
    MetricDefinition, MetricSection, ProviderDefinition, ProviderSnapshot, UsageHistory,
};

use self::{
    auth::load_token,
    client::{AntigravityClient, CloudOutcome, RefreshOutcome},
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
    #[error("Antigravity usage is temporarily unavailable. Try again shortly.")]
    Unavailable,
}

pub struct AntigravityProvider {
    client: AntigravityClient,
    refreshed_access_token: Mutex<Option<CachedAccessToken>>,
}

#[derive(Clone)]
struct CachedAccessToken {
    value: String,
    credential_fingerprint: [u8; 32],
}

impl AntigravityProvider {
    pub fn new() -> Result<Self, AntigravityError> {
        Ok(Self {
            client: AntigravityClient::new()?,
            refreshed_access_token: Mutex::new(None),
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

        let keychain = load_token();
        let source_fingerprint = keychain
            .as_ref()
            .and_then(|token| auth::credential_fingerprint(token.refresh_token.as_deref()));
        let access_token =
            matching_cached_access_token(&self.refreshed_access_token, source_fingerprint).or_else(
                || {
                    keychain.as_ref().and_then(|token| {
                        let usable = token.expiry.is_none_or(|expiry| {
                            expiry > Utc::now() + chrono::Duration::seconds(60)
                        });
                        usable.then(|| token.access_token.clone()).flatten()
                    })
                },
            );
        let tried_credentials = access_token.is_some();
        if let Some(token) = access_token {
            match self.fetch_remote(&token) {
                Ok(snapshot) => return Ok(snapshot),
                Err(AntigravityError::AuthExpired) => {}
                Err(error) => return Err(error),
            }
        }

        if let Some(refresh_token) = keychain
            .as_ref()
            .and_then(|token| token.refresh_token.as_deref())
        {
            match self.client.refresh_google_token(refresh_token) {
                RefreshOutcome::Refreshed { access_token } => {
                    crate::app_info!("auth:antigravity", "token refresh succeeded");
                    let snapshot = self.fetch_remote(&access_token)?;
                    if let (Some(credential_fingerprint), Ok(mut cached)) =
                        (source_fingerprint, self.refreshed_access_token.lock())
                    {
                        *cached = Some(CachedAccessToken {
                            value: access_token,
                            credential_fingerprint,
                        });
                    }
                    return Ok(snapshot);
                }
                RefreshOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
                RefreshOutcome::Unavailable => return Err(AntigravityError::Unavailable),
            }
        }
        if tried_credentials || keychain.is_some() {
            Err(AntigravityError::AuthExpired)
        } else {
            Err(AntigravityError::NotSignedIn)
        }
    }

    fn fetch_remote(&self, token: &str) -> Result<ProviderSnapshot, AntigravityError> {
        match self.client.cloud_code(QUOTA_SUMMARY_PATH, token, json!({})) {
            CloudOutcome::Ok(value) => {
                if let Some(quotas) = parse_quota_summary(&value) {
                    return Ok(snapshot(self.load_remote_plan(token), quotas));
                }
            }
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => {}
        }

        match self.client.cloud_code(FETCH_MODELS_PATH, token, json!({})) {
            CloudOutcome::Ok(value) => {
                let quotas = build_legacy_quotas(parse_cloud_models(&value));
                if !quotas.is_empty() {
                    return Ok(snapshot(self.load_remote_plan(token), quotas));
                }
            }
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => {}
        }

        let (plan, project) = match self
            .client
            .cloud_code(LOAD_CODE_ASSIST_PATH, token, json!({}))
        {
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
        let mut quota = self.client.cloud_code(RETRIEVE_QUOTA_PATH, token, body);
        if matches!(quota, CloudOutcome::Unavailable) && project.is_some() {
            quota = self
                .client
                .cloud_code(RETRIEVE_QUOTA_PATH, token, json!({}));
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
            .cloud_code(LOAD_CODE_ASSIST_PATH, token, json!({}))
        {
            CloudOutcome::Ok(value) => remote_plan(&value),
            _ => None,
        }
    }
}

fn matching_cached_access_token(
    cache: &Mutex<Option<CachedAccessToken>>,
    source_fingerprint: Option<[u8; 32]>,
) -> Option<String> {
    let mut cache = cache.lock().ok()?;
    let matches = cache
        .as_ref()
        .is_some_and(|cached| Some(cached.credential_fingerprint) == source_fingerprint);
    if matches {
        cache.as_ref().map(|cached| cached.value.clone())
    } else {
        *cache = None;
        None
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
        notices: Vec::new(),
        usage: UsageHistory::default(),
        warnings: Vec::new(),
        refreshed_at: Utc::now(),
    }
}

impl crate::providers::UsageProvider for AntigravityProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        auth::has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, crate::providers::ProviderError> {
        self.refresh_inner().map_err(|error| {
            use crate::models::ProviderErrorKind as Kind;

            let kind = match error {
                AntigravityError::NotSignedIn | AntigravityError::AuthExpired => {
                    Kind::Authentication
                }
                AntigravityError::Unavailable => Kind::Network,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{matching_cached_access_token, CachedAccessToken};

    #[test]
    fn cached_access_token_is_bound_to_its_refresh_credential() {
        let cache = Mutex::new(Some(CachedAccessToken {
            value: "derived-access".into(),
            credential_fingerprint: [1; 32],
        }));
        assert_eq!(
            matching_cached_access_token(&cache, Some([1; 32])).as_deref(),
            Some("derived-access")
        );
        assert!(matching_cached_access_token(&cache, Some([2; 32])).is_none());
        assert!(cache.lock().unwrap().is_none());
    }
}
