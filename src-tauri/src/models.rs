use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApiKeyStatus {
    NotSet,
    FromEnvironment,
    FromConfig,
    Saved,
    OverrideActive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyState {
    pub provider_id: String,
    pub status: ApiKeyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub id: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub period_seconds: u64,
    #[serde(default)]
    pub format: QuotaFormat,
    #[serde(default)]
    pub used_value: Option<f64>,
    #[serde(default)]
    pub limit_value: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MetricValueKind {
    Count,
    Dollars,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricValue {
    pub number: f64,
    pub kind: MetricValueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValueMetric {
    pub id: String,
    pub label: String,
    pub values: Vec<MetricValue>,
    #[serde(default)]
    pub expiries_at: Vec<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderNoticeTone {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderNotice {
    pub id: String,
    pub title: String,
    pub message: String,
    pub tone: ProviderNoticeTone,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuotaFormat {
    #[default]
    Percent,
    Dollars,
    Count,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    pub tokens: u64,
    pub estimated_cost_usd: Option<f64>,
    #[serde(default = "default_true")]
    pub cost_estimated: bool,
    pub estimate_complete: bool,
    #[serde(default)]
    pub model_breakdown: Option<ModelUsageBreakdown>,
    #[serde(default)]
    pub unknown_models: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageEntry {
    pub model: String,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<ModelUsageVariant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageVariant {
    pub model: String,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageBreakdown {
    pub models: Vec<ModelUsageEntry>,
    pub source_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub date: String,
    pub tokens: u64,
    pub estimated_cost_usd: Option<f64>,
    pub estimate_complete: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistory {
    pub today: Option<UsagePeriod>,
    pub yesterday: Option<UsagePeriod>,
    pub last_30_days: Option<UsagePeriod>,
    pub daily: Vec<DailyUsage>,
    pub unknown_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider_id: String,
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    #[serde(default)]
    pub value_metrics: Vec<ValueMetric>,
    #[serde(default)]
    pub notices: Vec<ProviderNotice>,
    pub usage: UsageHistory,
    pub warnings: Vec<String>,
    pub refreshed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotSource {
    None,
    Cache,
    Live,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderErrorKind {
    Authentication,
    Permission,
    RateLimited,
    Network,
    InvalidResponse,
    CredentialStorage,
    LocalData,
    Storage,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderViewState {
    pub snapshot: Option<ProviderSnapshot>,
    pub source: SnapshotSource,
    pub refreshing: bool,
    pub stale: bool,
    pub error: Option<String>,
    pub error_kind: Option<ProviderErrorKind>,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

impl Default for ProviderViewState {
    fn default() -> Self {
        Self {
            snapshot: None,
            source: SnapshotSource::None,
            refreshing: false,
            stale: false,
            error: None,
            error_kind: None,
            last_attempt_at: None,
        }
    }
}

impl ProviderViewState {
    pub fn from_cache(snapshot: ProviderSnapshot) -> Self {
        Self {
            snapshot: Some(snapshot),
            source: SnapshotSource::Cache,
            stale: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MetricSection {
    AlwaysVisible,
    OnDemand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MetricSource {
    Quota {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "sessionWindow")]
        session_window: bool,
    },
    QuotaOrValue {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "sessionWindow")]
        session_window: bool,
    },
    Value {
        #[serde(rename = "sourceId")]
        source_id: String,
    },
    Usage {
        period: UsagePeriodSelection,
    },
    Trend,
}

impl MetricSource {
    pub fn source_id(&self) -> Option<&str> {
        match self {
            Self::Quota { source_id, .. }
            | Self::QuotaOrValue { source_id, .. }
            | Self::Value { source_id } => Some(source_id),
            Self::Usage { .. } | Self::Trend => None,
        }
    }

    pub fn session_window(&self) -> bool {
        matches!(
            self,
            Self::Quota {
                session_window: true,
                ..
            } | Self::QuotaOrValue {
                session_window: true,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayMetricDefinition {
    pub short_label: String,
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MetricDefinition {
    pub id: String,
    pub label: String,
    pub source: MetricSource,
    pub pinnable: bool,
    pub default_enabled: bool,
    pub default_section: MetricSection,
    pub default_pinned: bool,
    pub tray: Option<TrayMetricDefinition>,
}

impl MetricDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        source: MetricSource,
        pinnable: bool,
        default_enabled: bool,
        default_section: MetricSection,
        default_pinned: bool,
        tray_short_label: Option<&str>,
        tray_suffix: Option<&str>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            source,
            pinnable,
            default_enabled,
            default_section,
            default_pinned,
            tray: tray_short_label.map(|short_label| TrayMetricDefinition {
                short_label: short_label.into(),
                suffix: tray_suffix.map(str::to_owned),
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quota(
        id: &str,
        label: &str,
        source_id: &str,
        session_window: bool,
        default_enabled: bool,
        default_section: MetricSection,
        default_pinned: bool,
        tray_short_label: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            MetricSource::Quota {
                source_id: source_id.into(),
                session_window,
            },
            true,
            default_enabled,
            default_section,
            default_pinned,
            Some(tray_short_label),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quota_or_value(
        id: &str,
        label: &str,
        source_id: &str,
        default_enabled: bool,
        default_section: MetricSection,
        default_pinned: bool,
        tray_short_label: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            MetricSource::QuotaOrValue {
                source_id: source_id.into(),
                session_window: false,
            },
            true,
            default_enabled,
            default_section,
            default_pinned,
            Some(tray_short_label),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn value(
        id: &str,
        label: &str,
        source_id: &str,
        default_enabled: bool,
        default_section: MetricSection,
        default_pinned: bool,
        tray_short_label: &str,
        tray_suffix: Option<&str>,
    ) -> Self {
        Self::new(
            id,
            label,
            MetricSource::Value {
                source_id: source_id.into(),
            },
            true,
            default_enabled,
            default_section,
            default_pinned,
            Some(tray_short_label),
            tray_suffix,
        )
    }

    pub fn usage(
        id: &str,
        label: &str,
        period: UsagePeriodSelection,
        default_section: MetricSection,
        tray_short_label: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            MetricSource::Usage { period },
            true,
            true,
            default_section,
            false,
            Some(tray_short_label),
            None,
        )
    }

    pub fn trend(id: &str) -> Self {
        Self::new(
            id,
            "Usage Trend",
            MetricSource::Trend,
            false,
            true,
            MetricSection::AlwaysVisible,
            false,
            None,
            None,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLink {
    pub label: String,
    pub url: String,
}

impl ProviderLink {
    pub fn new(label: &str, url: &str) -> Self {
        Self {
            label: label.into(),
            url: url.into(),
        }
    }

    pub fn visible(&self) -> Option<Self> {
        let label = self.label.trim();
        let url = self.url.trim();
        if label.is_empty()
            || url.is_empty()
            || !(url.starts_with("https://") || url.starts_with("http://"))
        {
            return None;
        }
        Some(Self::new(label, url))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefinition {
    pub id: String,
    pub display_name: String,
    pub short_name: String,
    pub fallback_enabled: bool,
    pub local_usage_source_note: Option<String>,
    #[serde(default)]
    pub links: Vec<ProviderLink>,
    pub metrics: Vec<MetricDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalog {
    pub providers: Vec<ProviderDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MetricLayout {
    pub id: String,
    pub enabled: bool,
    pub section: MetricSection,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLayout {
    pub id: String,
    pub enabled: bool,
    pub detected: bool,
    pub expanded: bool,
    pub metrics: Vec<MetricLayout>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DensityPreference {
    Default,
    Compact,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MenuBarStyle {
    Text,
    Bars,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UsageDisplay {
    Used,
    Left,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResetDisplay {
    Countdown,
    Exact,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TimeFormatPreference {
    #[default]
    System,
    TwelveHour,
    TwentyFourHour,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Debug = 3,
    #[default]
    #[serde(other)]
    Info = 2,
}

impl LogLevel {
    pub fn from_severity(value: u8) -> Self {
        match value {
            0 => Self::Error,
            1 => Self::Warn,
            3 => Self::Debug,
            _ => Self::Info,
        }
    }

    pub fn log_label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TotalSpendMetric {
    Cost,
    CostPerMillion,
    Tokens,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UsagePeriodSelection {
    Today,
    Yesterday,
    Last30Days,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationPreferences {
    pub almost_out: bool,
    pub cutting_it_close: bool,
    pub will_run_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub providers: Vec<ProviderLayout>,
    pub known_provider_ids: Vec<String>,
    pub show_total_spend: bool,
    pub theme: ThemePreference,
    pub density: DensityPreference,
    pub menu_bar_style: MenuBarStyle,
    pub usage_display: UsageDisplay,
    pub reset_display: ResetDisplay,
    pub time_format: TimeFormatPreference,
    pub always_show_pacing: bool,
    pub launch_at_login: bool,
    pub auto_check_updates: bool,
    pub dismissed_update_version: Option<String>,
    pub last_update_check_at: Option<DateTime<Utc>>,
    pub global_shortcut: Option<String>,
    pub log_level: LogLevel,
    pub notifications: NotificationPreferences,
    pub total_spend_metric: TotalSpendMetric,
    pub total_spend_period: UsagePeriodSelection,
    pub detection_notice_dismissed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 5,
            providers: Vec::new(),
            known_provider_ids: Vec::new(),
            show_total_spend: true,
            theme: ThemePreference::System,
            density: DensityPreference::Default,
            menu_bar_style: MenuBarStyle::Text,
            usage_display: UsageDisplay::Left,
            reset_display: ResetDisplay::Countdown,
            time_format: TimeFormatPreference::System,
            always_show_pacing: false,
            launch_at_login: false,
            auto_check_updates: true,
            dismissed_update_version: None,
            last_update_check_at: None,
            global_shortcut: None,
            log_level: LogLevel::Info,
            notifications: NotificationPreferences::default(),
            total_spend_metric: TotalSpendMetric::Cost,
            total_spend_period: UsagePeriodSelection::Today,
            detection_notice_dismissed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsViewState {
    pub settings: AppSettings,
    pub notification_permission: String,
    pub integration_error: Option<String>,
    pub standalone_window: bool,
    pub platform_summary: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeyStatus, AppSettings, LogLevel, ProviderApiKeyState, ProviderErrorKind, ProviderLink,
        ProviderSnapshot, ProviderViewState, UsagePeriod,
    };

    #[test]
    fn older_settings_default_new_update_state_fields() {
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("dismissedUpdateVersion");
        object.remove("lastUpdateCheckAt");
        object.remove("logLevel");

        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.dismissed_update_version, None);
        assert_eq!(settings.last_update_check_at, None);
        assert_eq!(settings.log_level, LogLevel::Info);
    }

    #[test]
    fn unknown_persisted_log_levels_fall_back_to_info() {
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        value["logLevel"] = serde_json::json!("trace");
        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.log_level, LogLevel::Info);
    }

    #[test]
    fn provider_error_kind_uses_the_frontend_contract_name() {
        let state = ProviderViewState {
            error: Some("Could not connect to the provider.".into()),
            error_kind: Some(ProviderErrorKind::Network),
            ..ProviderViewState::default()
        };

        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["errorKind"], "network");
    }

    #[test]
    fn api_key_state_exposes_status_without_a_secret_field() {
        let value = serde_json::to_value(ProviderApiKeyState {
            provider_id: "openrouter".into(),
            status: ApiKeyStatus::OverrideActive,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "providerId": "openrouter",
                "status": "overrideActive"
            })
        );
    }

    #[test]
    fn cached_usage_periods_default_to_local_cost_estimates() {
        let period: UsagePeriod = serde_json::from_str(
            r#"{"tokens":42,"estimatedCostUsd":0.12,"estimateComplete":true,"unknownModels":[]}"#,
        )
        .unwrap();
        assert!(period.cost_estimated);
    }

    #[test]
    fn cached_snapshots_default_new_dynamic_rows() {
        let snapshot: ProviderSnapshot = serde_json::from_str(
            r#"{
                "providerId":"codex",
                "plan":null,
                "quotas":[],
                "usage":{"today":null,"yesterday":null,"last30Days":null,"daily":[],"unknownModels":[]},
                "warnings":[],
                "refreshedAt":"2026-07-15T00:00:00Z"
            }"#,
        )
        .unwrap();
        assert!(snapshot.value_metrics.is_empty());
        assert!(snapshot.notices.is_empty());
    }

    #[test]
    fn provider_link_visibility_matches_the_trimmed_http_contract() {
        let links = [
            ProviderLink::new(" Status ", " https://status.example.com/ "),
            ProviderLink::new("HTTP", "http://example.com/dashboard"),
            ProviderLink::new("", "https://example.com/"),
            ProviderLink::new("No URL", " "),
            ProviderLink::new("FTP", "ftp://example.com/"),
            ProviderLink::new("JS", "javascript:alert(1)"),
            ProviderLink::new("Mail", "mailto:a@example.com"),
            ProviderLink::new("No scheme", "example.com"),
        ];

        assert_eq!(
            links
                .iter()
                .filter_map(ProviderLink::visible)
                .collect::<Vec<_>>(),
            [
                ProviderLink::new("Status", "https://status.example.com/"),
                ProviderLink::new("HTTP", "http://example.com/dashboard"),
            ]
        );
    }
}
