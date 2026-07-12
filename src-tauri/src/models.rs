use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuotaFormat {
    #[default]
    Percent,
    Dollars,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    pub tokens: u64,
    pub estimated_cost_usd: Option<f64>,
    pub estimate_complete: bool,
    #[serde(default)]
    pub model_breakdown: Option<ModelUsageBreakdown>,
    #[serde(default)]
    pub unknown_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageEntry {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderViewState {
    pub snapshot: Option<ProviderSnapshot>,
    pub source: SnapshotSource,
    pub refreshing: bool,
    pub stale: bool,
    pub error: Option<String>,
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
    pub notifications: NotificationPreferences,
    pub total_spend_metric: TotalSpendMetric,
    pub total_spend_period: UsagePeriodSelection,
    pub detection_notice_dismissed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 4,
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
    use super::AppSettings;

    #[test]
    fn older_settings_default_new_update_state_fields() {
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("dismissedUpdateVersion");
        object.remove("lastUpdateCheckAt");

        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.dismissed_update_version, None);
        assert_eq!(settings.last_update_check_at, None);
    }
}
