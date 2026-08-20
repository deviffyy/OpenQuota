mod auth;
mod client;
mod mapper;

use chrono::Utc;
use thiserror::Error;

use crate::models::{
    MetricDefinition, MetricSection, ProviderDefinition, ProviderErrorKind, ProviderLink,
    ProviderSnapshot, UsageHistory,
};

use self::{auth::CommandCodeAuthStore, client::CommandCodeClient, mapper::map_usage};

use super::{ProviderError, UsageProvider};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "commandcode".into(),
        display_name: "Command Code".into(),
        short_name: "CC".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        links: vec![ProviderLink::new("Usage", "https://commandcode.ai/usage")],
        metrics: vec![
            MetricDefinition::quota(
                "commandcode.session",
                "Session",
                "session",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "S",
            ),
            MetricDefinition::quota(
                "commandcode.weekly",
                "Weekly",
                "weekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "W",
            ),
            MetricDefinition::value(
                "commandcode.monthly",
                "Monthly Credits",
                "monthlyCredits",
                true,
                MetricSection::OnDemand,
                false,
                "M",
                None,
            ),
            MetricDefinition::value(
                "commandcode.extra",
                "Extra Credits",
                "extraCredits",
                true,
                MetricSection::OnDemand,
                false,
                "E",
                None,
            ),
        ],
    }
}

#[derive(Debug, Error)]
pub(crate) enum CommandCodeError {
    #[error("Command Code is not logged in. Run `command-code login`.")]
    NotLoggedIn,
    #[error("Command Code login data is invalid or expired. Run `command-code login` again.")]
    InvalidAuth,
    #[error("Could not reach Command Code. Check your internet connection.")]
    ConnectionFailed,
    #[error("Command Code returned an invalid usage response.")]
    InvalidResponse,
    #[error("Command Code usage request failed (HTTP {0}).")]
    RequestFailed(u16),
}

impl From<CommandCodeError> for ProviderError {
    fn from(error: CommandCodeError) -> Self {
        let kind = match error {
            CommandCodeError::NotLoggedIn | CommandCodeError::InvalidAuth => {
                ProviderErrorKind::Authentication
            }
            CommandCodeError::ConnectionFailed => ProviderErrorKind::Network,
            CommandCodeError::RequestFailed(429) => ProviderErrorKind::RateLimited,
            CommandCodeError::RequestFailed(_) | CommandCodeError::InvalidResponse => {
                ProviderErrorKind::InvalidResponse
            }
        };
        ProviderError::from_display(kind, error)
    }
}

pub struct CommandCodeProvider {
    auth: CommandCodeAuthStore,
    client: CommandCodeClient,
}

impl CommandCodeProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            auth: CommandCodeAuthStore::new(),
            client: CommandCodeClient::new().map_err(ProviderError::from)?,
        })
    }
}

impl UsageProvider for CommandCodeProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        self.auth.has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
        let api_key = self.auth.load().map_err(ProviderError::from)?;
        let (credits, subscription) = self.client.fetch(&api_key).map_err(ProviderError::from)?;
        let mapped = map_usage(&credits, &subscription).map_err(ProviderError::from)?;
        Ok(ProviderSnapshot {
            provider_id: "commandcode".into(),
            plan: mapped.plan,
            quotas: mapped.quotas,
            value_metrics: mapped.value_metrics,
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        })
    }
}
