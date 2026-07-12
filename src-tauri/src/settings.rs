use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use crate::{
    models::{AppSettings, MetricLayout, MetricSection, ProviderLayout, SettingsViewState},
    storage::Storage,
};

pub const CLAUDE_PROVIDER_ID: &str = "claude";
pub const CODEX_PROVIDER_ID: &str = "codex";
pub const ANTIGRAVITY_PROVIDER_ID: &str = "antigravity";
pub const MAX_PINS_PER_PROVIDER: usize = 2;

#[derive(Clone, Copy)]
struct MetricSpec {
    id: &'static str,
    section: MetricSection,
    enabled: bool,
    pinned: bool,
}

const CLAUDE_METRICS: [MetricSpec; 9] = [
    metric("claude.session", MetricSection::AlwaysVisible, true, true),
    metric("claude.weekly", MetricSection::AlwaysVisible, true, true),
    metric("claude.sonnet", MetricSection::OnDemand, false, false),
    metric("claude.fable", MetricSection::OnDemand, false, false),
    metric("claude.extra", MetricSection::AlwaysVisible, true, false),
    metric("claude.trend", MetricSection::AlwaysVisible, true, false),
    metric("claude.today", MetricSection::OnDemand, true, false),
    metric("claude.yesterday", MetricSection::OnDemand, true, false),
    metric("claude.last30", MetricSection::OnDemand, true, false),
];

const CODEX_METRICS: [MetricSpec; 6] = [
    metric("codex.session", MetricSection::AlwaysVisible, true, true),
    metric("codex.weekly", MetricSection::AlwaysVisible, true, true),
    metric("codex.trend", MetricSection::AlwaysVisible, true, false),
    metric("codex.today", MetricSection::OnDemand, true, false),
    metric("codex.yesterday", MetricSection::OnDemand, true, false),
    metric("codex.last30", MetricSection::OnDemand, true, false),
];

const ANTIGRAVITY_METRICS: [MetricSpec; 4] = [
    metric(
        "antigravity.geminiPro",
        MetricSection::AlwaysVisible,
        true,
        true,
    ),
    metric(
        "antigravity.geminiWeekly",
        MetricSection::AlwaysVisible,
        true,
        true,
    ),
    metric("antigravity.claude", MetricSection::OnDemand, true, false),
    metric(
        "antigravity.claudeWeekly",
        MetricSection::OnDemand,
        true,
        false,
    ),
];

const fn metric(
    id: &'static str,
    section: MetricSection,
    enabled: bool,
    pinned: bool,
) -> MetricSpec {
    MetricSpec {
        id,
        section,
        enabled,
        pinned,
    }
}

fn provider_specs() -> [(&'static str, &'static [MetricSpec]); 3] {
    [
        (CLAUDE_PROVIDER_ID, &CLAUDE_METRICS),
        (CODEX_PROVIDER_ID, &CODEX_METRICS),
        (ANTIGRAVITY_PROVIDER_ID, &ANTIGRAVITY_METRICS),
    ]
}

pub struct SettingsService {
    storage: Arc<Storage>,
    settings: RwLock<AppSettings>,
}

impl SettingsService {
    pub fn new(storage: Arc<Storage>, detected: &HashSet<String>) -> Self {
        let mut settings = storage
            .load_settings()
            .ok()
            .flatten()
            .unwrap_or_else(|| default_settings(detected));
        normalize(&mut settings, detected);
        let _ = storage.save_settings(&settings);
        Self {
            storage,
            settings: RwLock::new(settings),
        }
    }

    pub fn get(&self) -> AppSettings {
        self.settings
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    pub fn update(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        let detected = self
            .get()
            .providers
            .iter()
            .filter(|provider| provider.detected)
            .map(|provider| provider.id.clone())
            .collect::<HashSet<_>>();
        normalize(&mut settings, &detected);
        self.storage
            .save_settings(&settings)
            .map_err(|_| "OpenQuota settings could not be saved.".to_owned())?;
        self.settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?
            .clone_from(&settings);
        Ok(settings)
    }

    pub fn detected_provider_ids(&self) -> HashSet<String> {
        self.get()
            .providers
            .iter()
            .filter(|provider| provider.detected)
            .map(|provider| provider.id.clone())
            .collect()
    }

    pub fn view_state(
        &self,
        notification_permission: impl Into<String>,
        integration_error: Option<String>,
        standalone_window: bool,
        platform_summary: Option<String>,
    ) -> SettingsViewState {
        SettingsViewState {
            settings: self.get(),
            notification_permission: notification_permission.into(),
            integration_error,
            standalone_window,
            platform_summary,
        }
    }
}

pub fn default_settings(detected: &HashSet<String>) -> AppSettings {
    let mut settings = AppSettings {
        known_provider_ids: provider_specs()
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect(),
        providers: provider_specs()
            .iter()
            .map(|(id, specs)| default_provider(id, specs, detected.contains(*id)))
            .collect(),
        ..AppSettings::default()
    };
    if !settings.providers.iter().any(|provider| provider.enabled) {
        if let Some(codex) = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == CODEX_PROVIDER_ID)
        {
            codex.enabled = true;
        }
    }
    settings
}

pub fn normalize(settings: &mut AppSettings, detected: &HashSet<String>) {
    let migrating_to_multi_provider = settings.schema_version < 3;
    settings.schema_version = 4;
    settings.dismissed_update_version = settings
        .dismissed_update_version
        .take()
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty());
    settings.global_shortcut = settings
        .global_shortcut
        .take()
        .map(|shortcut| shortcut.trim().to_owned())
        .filter(|shortcut| !shortcut.is_empty());

    if settings.known_provider_ids.is_empty() {
        settings.known_provider_ids = settings
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect();
    }

    let specs = provider_specs();
    let mut normalized = Vec::new();
    for mut provider in settings.providers.clone() {
        let Some((id, metric_specs)) = specs.iter().find(|(id, _)| *id == provider.id) else {
            continue;
        };
        if normalized
            .iter()
            .any(|known: &ProviderLayout| known.id == provider.id)
        {
            continue;
        }
        let was_known = settings.known_provider_ids.iter().any(|known| known == id);
        if !was_known {
            provider.enabled = detected.contains(*id);
            settings.known_provider_ids.push((*id).into());
        }
        provider.detected = detected.contains(*id);
        normalize_metrics(&mut provider.metrics, metric_specs);
        normalized.push(provider);
    }
    for (id, metric_specs) in specs {
        if normalized.iter().any(|provider| provider.id == id) {
            continue;
        }
        let was_known = settings.known_provider_ids.iter().any(|known| known == id);
        let mut provider = default_provider(id, metric_specs, detected.contains(id));
        provider.enabled = !was_known && detected.contains(id);
        settings.known_provider_ids.push(id.into());
        normalized.push(provider);
    }
    if migrating_to_multi_provider {
        let order = provider_specs();
        normalized.sort_by_key(|provider| {
            order
                .iter()
                .position(|(id, _)| *id == provider.id)
                .unwrap_or(usize::MAX)
        });
    }
    settings.providers = normalized;
    settings.known_provider_ids.sort();
    settings.known_provider_ids.dedup();
}

fn default_provider(id: &str, specs: &[MetricSpec], detected: bool) -> ProviderLayout {
    ProviderLayout {
        id: id.into(),
        enabled: detected,
        detected,
        expanded: false,
        metrics: specs
            .iter()
            .map(|spec| MetricLayout {
                id: spec.id.into(),
                enabled: spec.enabled,
                section: spec.section,
                pinned: spec.pinned,
            })
            .collect(),
    }
}

fn normalize_metrics(metrics: &mut Vec<MetricLayout>, specs: &[MetricSpec]) {
    let mut normalized = Vec::with_capacity(specs.len());
    for metric in metrics.iter() {
        if specs.iter().any(|spec| spec.id == metric.id)
            && !normalized
                .iter()
                .any(|known: &MetricLayout| known.id == metric.id)
        {
            normalized.push(metric.clone());
        }
    }
    for spec in specs {
        if !normalized.iter().any(|metric| metric.id == spec.id) {
            normalized.push(MetricLayout {
                id: spec.id.into(),
                enabled: spec.enabled,
                section: spec.section,
                pinned: spec.pinned,
            });
        }
    }
    let mut pin_count = 0;
    for metric in &mut normalized {
        let pinnable = !metric.id.ends_with(".trend");
        metric.pinned &= metric.enabled && pinnable && pin_count < MAX_PINS_PER_PROVIDER;
        if metric.pinned {
            pin_count += 1;
        }
    }
    if !normalized
        .iter()
        .any(|metric| metric.enabled && metric.section == MetricSection::AlwaysVisible)
    {
        if let Some(metric) = normalized.iter_mut().find(|metric| metric.enabled) {
            metric.section = MetricSection::AlwaysVisible;
        }
    }
    *metrics = normalized;
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use tempfile::tempdir;

    use crate::{models::MetricSection, storage::Storage};

    use super::{default_settings, normalize, SettingsService, MAX_PINS_PER_PROVIDER};

    #[test]
    fn normalization_preserves_order_and_enforces_pin_cap_per_provider() {
        let detected = HashSet::from(["codex".to_owned(), "claude".to_owned()]);
        let mut settings = default_settings(&detected);
        let metrics = &mut settings.providers[0].metrics;
        metrics.rotate_left(2);
        for metric in metrics.iter_mut() {
            metric.enabled = true;
            metric.pinned = true;
        }
        normalize(&mut settings, &detected);
        let metrics = &settings.providers[0].metrics;
        assert_eq!(
            metrics.iter().filter(|metric| metric.pinned).count(),
            MAX_PINS_PER_PROVIDER
        );
        assert!(metrics
            .iter()
            .find(|metric| metric.id.ends_with(".trend"))
            .is_none_or(|metric| !metric.pinned));
    }

    #[test]
    fn normalization_keeps_one_always_visible_metric() {
        let detected = HashSet::from(["codex".to_owned()]);
        let mut settings = default_settings(&detected);
        for metric in &mut settings.providers[1].metrics {
            metric.section = MetricSection::OnDemand;
        }
        normalize(&mut settings, &detected);
        assert!(settings.providers[1]
            .metrics
            .iter()
            .any(|metric| metric.enabled && metric.section == MetricSection::AlwaysVisible));
    }

    #[test]
    fn layout_and_preferences_survive_a_service_restart() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let detected = HashSet::from(["codex".to_owned(), "antigravity".to_owned()]);
        let first = SettingsService::new(storage.clone(), &detected);
        let mut settings = first.get();
        settings.density = crate::models::DensityPreference::Compact;
        settings.dismissed_update_version = Some("0.2.0".to_owned());
        settings.last_update_check_at = Some(chrono::Utc::now());
        settings.providers.rotate_left(1);
        settings.providers[1].metrics.rotate_right(1);
        let expected = first.update(settings).unwrap();
        let second = SettingsService::new(storage, &detected);
        assert_eq!(second.get(), expected);
    }

    #[test]
    fn new_detected_provider_is_enabled_once_without_overriding_later_choice() {
        let mut settings = default_settings(&HashSet::from(["codex".to_owned()]));
        settings.known_provider_ids.retain(|id| id != "antigravity");
        settings
            .providers
            .retain(|provider| provider.id != "antigravity");
        let detected = HashSet::from(["codex".to_owned(), "antigravity".to_owned()]);
        normalize(&mut settings, &detected);
        let antigravity = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "antigravity")
            .unwrap();
        assert!(antigravity.enabled);
        antigravity.enabled = false;
        normalize(&mut settings, &detected);
        assert!(
            !settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn schema_two_migration_uses_the_multi_provider_default_order() {
        let mut settings = default_settings(&HashSet::from(["codex".to_owned()]));
        settings.schema_version = 2;
        settings.known_provider_ids.clear();
        settings.providers.retain(|provider| provider.id == "codex");
        normalize(
            &mut settings,
            &HashSet::from(["codex".to_owned(), "antigravity".to_owned()]),
        );
        assert_eq!(settings.schema_version, 4);
        assert_eq!(
            settings
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex", "antigravity"]
        );
    }
}
