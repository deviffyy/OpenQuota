pub mod auth;
mod client;
mod local_usage;
mod mapper;

use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use reqwest::StatusCode;
use thiserror::Error;

use crate::{
    models::ProviderSnapshot,
    pricing::{ModelPricing, PricingStore},
    storage::Storage,
};

use self::{
    auth::{load_candidates, oauth_config, ClaudeCredential},
    client::ClaudeClient,
    local_usage::scan_local_usage,
    mapper::map_usage,
};

#[derive(Debug, Error)]
pub enum ClaudeError {
    #[error("Not logged in. Run `claude` to authenticate.")]
    NotLoggedIn,
    #[error(
        "Signed in to the Claude desktop app? OpenQuota needs a CLI login — run `claude` in a terminal and sign in once."
    )]
    DesktopAppOnly,
    #[error("Your Claude session expired. Run `claude` to sign in again.")]
    SessionExpired,
    #[error("Your Claude token expired. Run `claude` to sign in again.")]
    TokenExpired,
    #[error("Claude OAuth settings contain an invalid URL.")]
    InvalidOAuthUrl,
    #[error("Refreshed Claude credentials could not be saved.")]
    AuthWrite,
    #[error("Claude usage request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("Claude returned an invalid usage response.")]
    InvalidResponse,
    #[error("Could not connect to Claude. Check your internet connection.")]
    ConnectionFailed,
    #[error("Local Claude usage logs could not be processed.")]
    LocalUsage,
}

pub struct ClaudeProvider {
    storage: Arc<Storage>,
    pricing: Arc<PricingStore>,
    client: ClaudeClient,
    last_good: Mutex<Option<ProviderSnapshot>>,
    rate_limited_until: Mutex<Option<chrono::DateTime<Utc>>>,
}

impl ClaudeProvider {
    pub fn new(storage: Arc<Storage>, pricing: Arc<PricingStore>) -> Result<Self, ClaudeError> {
        Ok(Self {
            storage,
            pricing,
            client: ClaudeClient::new()?,
            last_good: Mutex::new(None),
            rate_limited_until: Mutex::new(None),
        })
    }

    fn refresh_inner(&self) -> Result<ProviderSnapshot, ClaudeError> {
        let candidates = load_candidates();
        if candidates.is_empty() {
            return Err(if auth::has_desktop_app_data() {
                ClaudeError::DesktopAppOnly
            } else {
                ClaudeError::NotLoggedIn
            });
        }
        let now = Utc::now();
        let config = oauth_config()?;
        let pricing = self.pricing.current();
        let mut last_auth_error = None;
        for mut credential in candidates {
            match self.refresh_candidate(&mut credential, &config, now, &pricing) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error @ (ClaudeError::SessionExpired | ClaudeError::TokenExpired)) => {
                    last_auth_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_auth_error.unwrap_or(ClaudeError::NotLoggedIn))
    }

    fn refresh_candidate(
        &self,
        credential: &mut ClaudeCredential,
        config: &auth::ClaudeOAuthConfig,
        now: chrono::DateTime<Utc>,
        pricing: &ModelPricing,
    ) -> Result<ProviderSnapshot, ClaudeError> {
        let mut warnings = Vec::new();
        let usage = scan_local_usage(&self.storage, now, pricing)?;

        if credential.inference_only {
            return Ok(ProviderSnapshot {
                provider_id: "claude".into(),
                plan: plan_name(credential),
                quotas: Vec::new(),
                value_metrics: Vec::new(),
                usage,
                warnings,
                refreshed_at: now,
            });
        }
        if !credential.has_profile_scope() {
            warnings.push(
                "Re-login for live usage. Run `claude` and sign in again to restore subscription limits."
                    .into(),
            );
            return Ok(ProviderSnapshot {
                provider_id: "claude".into(),
                plan: plan_name(credential),
                quotas: Vec::new(),
                value_metrics: Vec::new(),
                usage,
                warnings,
                refreshed_at: now,
            });
        }
        if credential.needs_refresh(now.timestamp_millis()) {
            refresh_credential(&self.client, credential, config, now, &mut warnings)?;
        }

        let cooldown_until = self
            .rate_limited_until
            .lock()
            .ok()
            .and_then(|value| *value)
            .filter(|until| now < *until);
        if let Some(until) = cooldown_until {
            let retry = until.signed_duration_since(now).num_seconds().max(0) as u64;
            if let Some(mut snapshot) = self.last_good.lock().ok().and_then(|value| value.clone()) {
                snapshot.usage = usage;
                snapshot.warnings.push(
                    "Claude live usage is rate limited; showing the last successful limits.".into(),
                );
                snapshot.refreshed_at = now;
                return Ok(snapshot);
            }
            warnings.push(format!(
                "Claude live usage is rate limited; retrying in about {} minutes.",
                retry.div_ceil(60)
            ));
            return Ok(ProviderSnapshot {
                provider_id: "claude".into(),
                plan: plan_name(credential),
                quotas: Vec::new(),
                value_metrics: Vec::new(),
                usage,
                warnings,
                refreshed_at: now,
            });
        }

        let token = credential.access_token().ok_or(ClaudeError::NotLoggedIn)?;
        let (mut status, mut body, mut retry_after) = self.client.fetch_usage(token, config)?;
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            refresh_credential(&self.client, credential, config, now, &mut warnings)?;
            let token = credential.access_token().ok_or(ClaudeError::TokenExpired)?;
            (status, body, retry_after) = self.client.fetch_usage(token, config)?;
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry = retry_after.unwrap_or(5 * 60);
            if let Ok(mut until) = self.rate_limited_until.lock() {
                *until = Some(now + Duration::seconds(retry as i64));
            }
            if let Some(mut snapshot) = self.last_good.lock().ok().and_then(|value| value.clone()) {
                snapshot.usage = usage;
                snapshot.warnings.push(format!(
                    "Claude live usage is rate limited; retrying in about {} minutes.",
                    retry.div_ceil(60)
                ));
                snapshot.refreshed_at = now;
                return Ok(snapshot);
            }
            warnings.push(format!(
                "Claude live usage is rate limited; retrying in about {} minutes.",
                retry.div_ceil(60)
            ));
            return Ok(ProviderSnapshot {
                provider_id: "claude".into(),
                plan: plan_name(credential),
                quotas: Vec::new(),
                value_metrics: Vec::new(),
                usage,
                warnings,
                refreshed_at: now,
            });
        }
        self.build_snapshot(status, &body, credential, usage, warnings, now)
    }

    fn build_snapshot(
        &self,
        status: StatusCode,
        body: &serde_json::Value,
        credential: &ClaudeCredential,
        usage: crate::models::UsageHistory,
        warnings: Vec<String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<ProviderSnapshot, ClaudeError> {
        let mapped = map_usage(status, body, &credential.oauth)?;
        let snapshot = ProviderSnapshot {
            provider_id: "claude".into(),
            plan: mapped.plan,
            quotas: mapped.quotas,
            value_metrics: mapped.value_metrics,
            usage,
            warnings,
            refreshed_at: now,
        };
        if let Ok(mut last) = self.last_good.lock() {
            *last = Some(snapshot.clone());
        }
        if let Ok(mut until) = self.rate_limited_until.lock() {
            *until = None;
        }
        Ok(snapshot)
    }
}

fn refresh_credential(
    client: &ClaudeClient,
    credential: &mut ClaudeCredential,
    config: &auth::ClaudeOAuthConfig,
    now: chrono::DateTime<Utc>,
    warnings: &mut Vec<String>,
) -> Result<(), ClaudeError> {
    let refresh_token = credential
        .oauth
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(ClaudeError::TokenExpired)?;
    let refreshed = client.refresh_token(refresh_token, config)?;
    if credential
        .update_and_save(
            refreshed.access_token,
            refreshed.refresh_token,
            refreshed.expires_in,
            now.timestamp_millis(),
        )
        .is_err()
    {
        warnings.push(
            "The refreshed Claude login is active for this session but could not be saved.".into(),
        );
    }
    Ok(())
}

fn plan_name(credential: &ClaudeCredential) -> Option<String> {
    credential.oauth.subscription_type.as_ref().map(|value| {
        let mut chars = value.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    })
}

impl crate::providers::UsageProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn has_local_credentials(&self) -> bool {
        auth::has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, crate::providers::ProviderError> {
        self.refresh_inner().map_err(|error| {
            use crate::models::ProviderErrorKind as Kind;

            let kind = match error {
                ClaudeError::NotLoggedIn
                | ClaudeError::DesktopAppOnly
                | ClaudeError::SessionExpired
                | ClaudeError::TokenExpired => Kind::Authentication,
                ClaudeError::InvalidOAuthUrl | ClaudeError::InvalidResponse => {
                    Kind::InvalidResponse
                }
                ClaudeError::AuthWrite => Kind::CredentialStorage,
                ClaudeError::RequestFailed(429) => Kind::RateLimited,
                ClaudeError::RequestFailed(_) | ClaudeError::ConnectionFailed => Kind::Network,
                ClaudeError::LocalUsage => Kind::LocalData,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}
