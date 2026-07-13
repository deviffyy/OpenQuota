mod auth;
mod client;
mod discovery;
mod mapper;

use std::sync::Mutex;

use chrono::Utc;
use serde_json::{json, Value};
use thiserror::Error;

use crate::models::{ProviderSnapshot, UsageHistory};

use self::{
    auth::load_token,
    client::{AntigravityClient, CloudOutcome, RefreshOutcome},
    discovery::discover,
    mapper::{parse_plan, parse_quota_summary},
};

const QUOTA_SUMMARY_PATH: &str = "/v1internal:retrieveUserQuotaSummary";
const LOAD_CODE_ASSIST_PATH: &str = "/v1internal:loadCodeAssist";

#[derive(Debug, Error)]
pub enum AntigravityError {
    #[error("Start Antigravity or run `agy` and try again.")]
    NotSignedIn,
    #[error("Antigravity sign-in expired. Open Antigravity or run `agy` to refresh.")]
    AuthExpired,
    #[error("Antigravity usage is temporarily unavailable. Try again shortly.")]
    Unavailable,
    #[error("Antigravity returned an unsupported quota response.")]
    InvalidResponse,
}

pub struct AntigravityProvider {
    client: AntigravityClient,
    refreshed_access_token: Mutex<Option<String>>,
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
        }

        let keychain = load_token();
        let access_token = self
            .refreshed_access_token
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .or_else(|| {
                keychain.as_ref().and_then(|token| {
                    let usable = token
                        .expiry
                        .is_none_or(|expiry| expiry > Utc::now() + chrono::Duration::seconds(60));
                    usable.then(|| token.access_token.clone()).flatten()
                })
            });
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
                    let snapshot = self.fetch_remote(&access_token)?;
                    if let Ok(mut cached) = self.refreshed_access_token.lock() {
                        *cached = Some(access_token);
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
        let summary = match self.client.cloud_code(QUOTA_SUMMARY_PATH, token, json!({})) {
            CloudOutcome::Ok(value) => value,
            CloudOutcome::AuthFailed => return Err(AntigravityError::AuthExpired),
            CloudOutcome::Unavailable => return Err(AntigravityError::Unavailable),
        };
        let quotas = parse_quota_summary(&summary).ok_or(AntigravityError::InvalidResponse)?;
        let plan = match self
            .client
            .cloud_code(LOAD_CODE_ASSIST_PATH, token, json!({}))
        {
            CloudOutcome::Ok(value) => remote_plan(&value),
            _ => None,
        };
        Ok(snapshot(plan, quotas))
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
        usage: UsageHistory::default(),
        warnings: Vec::new(),
        refreshed_at: Utc::now(),
    }
}

impl crate::providers::UsageProvider for AntigravityProvider {
    fn id(&self) -> &'static str {
        "antigravity"
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
                AntigravityError::InvalidResponse => Kind::InvalidResponse,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}
