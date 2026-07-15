pub mod auth;
pub mod client;
pub mod local_usage;
pub mod mapper;

use std::sync::Arc;

use chrono::Utc;
use reqwest::StatusCode;
use thiserror::Error;

use crate::{
    models::{
        MetricDefinition, MetricSection, ProviderDefinition, ProviderLink, ProviderSnapshot,
        UsagePeriodSelection,
    },
    pricing::PricingStore,
    storage::Storage,
};

use self::{
    auth::CodexAuthState, client::CodexClient, local_usage::scan_local_usage, mapper::map_usage,
};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "codex".into(),
        display_name: "Codex".into(),
        short_name: "Cx".into(),
        fallback_enabled: true,
        local_usage_source_note: Some("From your Codex logs (estimated)".into()),
        links: vec![
            ProviderLink::new("Status", "https://status.openai.com/"),
            ProviderLink::new("Dashboard", "https://chatgpt.com/codex/settings/usage"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "codex.session",
                "Session",
                "session",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "S",
            ),
            MetricDefinition::quota(
                "codex.weekly",
                "Weekly",
                "weekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "W",
            ),
            MetricDefinition::quota(
                "codex.spark",
                "Spark",
                "spark",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "Sp",
            ),
            MetricDefinition::quota(
                "codex.sparkWeekly",
                "Spark Weekly",
                "sparkWeekly",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "SW",
            ),
            MetricDefinition::trend("codex.trend"),
            MetricDefinition::value(
                "codex.credits",
                "Extra Usage",
                "credits",
                true,
                MetricSection::OnDemand,
                false,
                "E",
                None,
            ),
            MetricDefinition::value(
                "codex.rateLimitResets",
                "Rate Limit Resets",
                "rateLimitResets",
                true,
                MetricSection::OnDemand,
                false,
                "R",
                Some("resets"),
            ),
            MetricDefinition::usage(
                "codex.today",
                "Today",
                UsagePeriodSelection::Today,
                MetricSection::OnDemand,
                "T",
            ),
            MetricDefinition::usage(
                "codex.yesterday",
                "Yesterday",
                UsagePeriodSelection::Yesterday,
                MetricSection::OnDemand,
                "Y",
            ),
            MetricDefinition::usage(
                "codex.last30",
                "Last 30 Days",
                UsagePeriodSelection::Last30Days,
                MetricSection::OnDemand,
                "M",
            ),
        ],
    }
}

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("Not logged in. Run `codex` to authenticate.")]
    NotLoggedIn,
    #[error(
        "Subscription usage is unavailable for API-key-only logins. Sign in to Codex with ChatGPT."
    )]
    ApiKeyOnly,
    #[error("Your Codex session expired. Run `codex` to sign in again.")]
    SessionExpired,
    #[error("Codex credentials changed while refreshing. Run `codex` to sign in again.")]
    TokenConflict,
    #[error("Your Codex session was revoked. Run `codex` to sign in again.")]
    TokenRevoked,
    #[error("Your Codex access token expired. Run `codex` to sign in again.")]
    TokenExpired,
    #[error("Codex auth data is invalid. Run `codex` to sign in again.")]
    InvalidAuth,
    #[error("Refreshed Codex credentials could not be saved.")]
    AuthWrite,
    #[error("Codex usage request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("Codex returned an invalid usage response.")]
    InvalidResponse,
    #[error("Could not connect to Codex. Check your internet connection.")]
    ConnectionFailed,
    #[error("Local Codex usage logs could not be processed.")]
    LocalUsage,
    #[error("OpenQuota cache is unavailable.")]
    Storage,
}

impl From<crate::storage::StorageError> for CodexError {
    fn from(_: crate::storage::StorageError) -> Self {
        Self::Storage
    }
}

pub struct CodexProvider {
    storage: Arc<Storage>,
    pricing: Arc<PricingStore>,
    client: CodexClient,
}

impl CodexProvider {
    pub fn new(storage: Arc<Storage>, pricing: Arc<PricingStore>) -> Result<Self, CodexError> {
        Ok(Self {
            storage,
            pricing,
            client: CodexClient::new()?,
        })
    }

    pub fn refresh(&self) -> Result<ProviderSnapshot, CodexError> {
        let now = Utc::now();
        let candidates = CodexAuthState::load_candidates()?;
        let mut last_auth_error = None;
        for mut auth in candidates {
            match self.refresh_candidate(&mut auth, now) {
                Ok(snapshot) => return Ok(snapshot),
                Err(
                    error @ (CodexError::SessionExpired
                    | CodexError::TokenConflict
                    | CodexError::TokenRevoked
                    | CodexError::TokenExpired),
                ) => last_auth_error = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last_auth_error.unwrap_or(CodexError::NotLoggedIn))
    }

    fn refresh_candidate(
        &self,
        auth: &mut CodexAuthState,
        now: chrono::DateTime<Utc>,
    ) -> Result<ProviderSnapshot, CodexError> {
        let mut warnings = Vec::new();

        if auth.needs_refresh(now) {
            if let Ok(live) = auth.reload() {
                *auth = live;
            }
        }
        if auth.needs_refresh(now) {
            self.refresh_access_token(auth, now, &mut warnings)?;
        }

        let mut response = self
            .client
            .fetch_usage(&auth.access_token, auth.account_id.as_deref())?;
        if matches!(
            response.status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.refresh_access_token(auth, now, &mut warnings)?;
            response = self
                .client
                .fetch_usage(&auth.access_token, auth.account_id.as_deref())?;
        }
        let reset_credits = if response.status.is_success() {
            self.client
                .fetch_reset_credits(&auth.access_token, auth.account_id.as_deref())
                .ok()
        } else {
            None
        };
        let mapped = map_usage(&response, reset_credits.as_ref(), now)?;
        let pricing = self.pricing.current();
        let usage = scan_local_usage(&self.storage, now, &pricing)?;
        Ok(ProviderSnapshot {
            provider_id: "codex".into(),
            plan: mapped.plan,
            quotas: mapped.quotas,
            value_metrics: mapped.value_metrics,
            notices: Vec::new(),
            usage,
            warnings,
            refreshed_at: now,
        })
    }

    fn refresh_access_token(
        &self,
        auth: &mut CodexAuthState,
        now: chrono::DateTime<Utc>,
        warnings: &mut Vec<String>,
    ) -> Result<(), CodexError> {
        let refresh_token = auth
            .refresh_token
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(CodexError::TokenExpired)?;
        let refreshed = self.client.refresh_token(refresh_token)?;
        if auth
            .update_and_save(
                refreshed.access_token,
                refreshed.refresh_token,
                refreshed.id_token,
                now,
            )
            .is_err()
        {
            warnings.push(
                "The refreshed Codex login is active for this session but could not be saved."
                    .into(),
            );
        }
        Ok(())
    }
}

impl crate::providers::UsageProvider for CodexProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        CodexAuthState::has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, crate::providers::ProviderError> {
        CodexProvider::refresh(self).map_err(|error| {
            use crate::models::ProviderErrorKind as Kind;

            let kind = match error {
                CodexError::NotLoggedIn
                | CodexError::SessionExpired
                | CodexError::TokenConflict
                | CodexError::TokenRevoked
                | CodexError::TokenExpired
                | CodexError::InvalidAuth => Kind::Authentication,
                CodexError::ApiKeyOnly => Kind::Permission,
                CodexError::AuthWrite => Kind::CredentialStorage,
                CodexError::RequestFailed(429) => Kind::RateLimited,
                CodexError::RequestFailed(_) | CodexError::ConnectionFailed => Kind::Network,
                CodexError::InvalidResponse => Kind::InvalidResponse,
                CodexError::LocalUsage => Kind::LocalData,
                CodexError::Storage => Kind::Storage,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}
