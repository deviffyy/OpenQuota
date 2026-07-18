mod database;
mod paths;
mod record;
mod scanner;
mod windows;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{
    models::{
        MetricDefinition, MetricSection, ProviderDefinition, ProviderErrorKind, ProviderLink,
        ProviderSnapshot, UsageHistory, UsagePeriodSelection,
    },
    pricing::PricingStore,
};

use self::{
    paths::OpenCodePaths,
    scanner::{OpenCodeUsageScanner, USAGE_SOURCE_NOTE},
    windows::OpenCodeWindows,
};

use super::{ProviderError, UsageProvider};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "opencode".into(),
        display_name: "OpenCode".into(),
        short_name: "OC".into(),
        fallback_enabled: false,
        local_usage_source_note: Some(USAGE_SOURCE_NOTE.into()),
        links: vec![ProviderLink::new("Dashboard", "https://opencode.ai/auth")],
        metrics: vec![
            MetricDefinition::quota(
                "opencode.session",
                "Session",
                "session",
                true,
                true,
                MetricSection::AlwaysVisible,
                false,
                "S",
            ),
            MetricDefinition::quota(
                "opencode.weekly",
                "Weekly",
                "weekly",
                false,
                true,
                MetricSection::AlwaysVisible,
                false,
                "W",
            ),
            MetricDefinition::quota(
                "opencode.monthly",
                "Monthly",
                "monthly",
                false,
                true,
                MetricSection::AlwaysVisible,
                false,
                "M",
            ),
            MetricDefinition::trend("opencode.trend"),
            MetricDefinition::usage(
                "opencode.today",
                "Today",
                UsagePeriodSelection::Today,
                MetricSection::OnDemand,
                "T",
            ),
            MetricDefinition::usage(
                "opencode.yesterday",
                "Yesterday",
                UsagePeriodSelection::Yesterday,
                MetricSection::OnDemand,
                "Y",
            ),
            MetricDefinition::usage(
                "opencode.last30",
                "Last 30 Days",
                UsagePeriodSelection::Last30Days,
                MetricSection::OnDemand,
                "30",
            ),
        ],
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenCodeError {
    #[error("OpenCode was not detected. Sign in to OpenCode Go or use OpenCode locally first.")]
    NotDetected,
    #[error("OpenCode login data could not be read. Sign in to OpenCode Go again.")]
    CredentialsUnreadable,
    #[error("The OpenCode data directory could not be read.")]
    DataDirectoryUnreadable,
    #[error("OpenCode local usage data is temporarily unavailable.")]
    DatabaseUnreadable,
}

impl From<OpenCodeError> for ProviderError {
    fn from(error: OpenCodeError) -> Self {
        let kind = match error {
            OpenCodeError::NotDetected => ProviderErrorKind::Authentication,
            OpenCodeError::CredentialsUnreadable => ProviderErrorKind::CredentialStorage,
            OpenCodeError::DataDirectoryUnreadable | OpenCodeError::DatabaseUnreadable => {
                ProviderErrorKind::LocalData
            }
        };
        ProviderError::new(kind, error.to_string())
    }
}

pub struct OpenCodeProvider {
    paths: OpenCodePaths,
    scanner: OpenCodeUsageScanner,
    pricing: Arc<PricingStore>,
    now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl OpenCodeProvider {
    pub fn new(pricing: Arc<PricingStore>) -> Self {
        let paths = OpenCodePaths::new();
        Self {
            scanner: OpenCodeUsageScanner::new(paths.clone()),
            paths,
            pricing,
            now: Arc::new(Utc::now),
        }
    }

    #[cfg(test)]
    fn with_dependencies(
        paths: OpenCodePaths,
        pricing: Arc<PricingStore>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            scanner: OpenCodeUsageScanner::new(paths.clone()),
            paths,
            pricing,
            now: Arc::new(move || now),
        }
    }

    fn refresh_snapshot(&self) -> Result<ProviderSnapshot, OpenCodeError> {
        let now = (self.now)();
        let (has_go_key, go_key_error) = match self.paths.go_api_key() {
            Ok(Some(_)) => (true, None),
            Ok(None) => (false, None),
            Err(error) => (false, Some(error)),
        };
        let pricing = self.pricing.current();
        let scan = self.scanner.scan(now, has_go_key, &pricing)?;

        let Some(scan) = scan else {
            if has_go_key {
                return Ok(snapshot(
                    Some("Go".into()),
                    OpenCodeWindows::compute(&[], None, now).quotas(),
                    UsageHistory::default(),
                    Vec::new(),
                    now,
                ));
            }
            return Err(go_key_error.unwrap_or(OpenCodeError::NotDetected));
        };

        let mut warnings = scan.warnings;
        if go_key_error.is_some() {
            warnings.push(
                "OpenCode Go login data could not be read; local database usage is still shown."
                    .into(),
            );
        }
        let (plan, quotas) = scan.go_windows.map_or_else(
            || (None, Vec::new()),
            |windows| (Some("Go".into()), windows.quotas()),
        );
        Ok(snapshot(plan, quotas, scan.usage, warnings, now))
    }
}

impl UsageProvider for OpenCodeProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        match self.paths.go_api_key() {
            Ok(Some(_)) | Err(OpenCodeError::CredentialsUnreadable) => true,
            Ok(None) | Err(_) => self.scanner.has_hosted_usage(),
        }
    }

    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
        self.refresh_snapshot().map_err(ProviderError::from)
    }
}

fn snapshot(
    plan: Option<String>,
    quotas: Vec<crate::models::QuotaWindow>,
    usage: UsageHistory,
    warnings: Vec<String>,
    refreshed_at: DateTime<Utc>,
) -> ProviderSnapshot {
    ProviderSnapshot {
        provider_id: "opencode".into(),
        plan,
        quotas,
        value_metrics: Vec::new(),
        status_metrics: Vec::new(),
        notices: Vec::new(),
        usage,
        warnings,
        refreshed_at,
    }
}

#[cfg(test)]
mod tests;
