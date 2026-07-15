use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use crate::{
    models::{
        AppSettings, MetricDefinition, MetricLayout, MetricSection, ProviderCatalog,
        ProviderDefinition, ProviderLayout, SettingsViewState,
    },
    providers::ProviderRegistry,
    storage::{Storage, StorageError},
};

pub const MAX_PINS_PER_PROVIDER: usize = 2;

#[derive(Debug, Clone)]
pub struct CredentialDetectionPlan {
    provider_ids: Vec<String>,
    auto_enable_provider_ids: HashSet<String>,
    replace_fallback: bool,
    enablement_revision: u64,
}

impl CredentialDetectionPlan {
    pub fn provider_ids(&self) -> &[String] {
        &self.provider_ids
    }
}

pub struct CredentialDetectionOutcome {
    pub settings: AppSettings,
    pub newly_enabled_provider_ids: Vec<String>,
}

pub struct SettingsService {
    storage: Arc<Storage>,
    registry: Arc<ProviderRegistry>,
    settings: RwLock<AppSettings>,
    enablement_revision: AtomicU64,
}

impl SettingsService {
    #[cfg(test)]
    fn new_for_test(
        storage: Arc<Storage>,
        registry: Arc<ProviderRegistry>,
        detected: &HashSet<String>,
    ) -> Result<Self, StorageError> {
        let mut settings = storage
            .load_settings()?
            .unwrap_or_else(|| default_settings(&registry, detected));
        normalize(&registry, &mut settings, detected);
        storage.save_settings(&settings)?;
        Ok(Self {
            storage,
            registry,
            settings: RwLock::new(settings),
            enablement_revision: AtomicU64::new(0),
        })
    }

    /// Loads settings immediately and returns a plan for non-blocking credential detection.
    ///
    /// Fresh installs render the registry fallback without waiting for credential stores. Existing
    /// installs keep their choices; only providers never seen before are eligible for automatic
    /// enablement after the probe completes.
    pub fn new_deferred(
        storage: Arc<Storage>,
        registry: Arc<ProviderRegistry>,
    ) -> Result<(Self, CredentialDetectionPlan), StorageError> {
        let saved = storage.load_settings()?;
        let fresh_install = saved.is_none();
        let mut settings = saved.unwrap_or_else(|| default_settings(&registry, &HashSet::new()));
        let detected = settings
            .providers
            .iter()
            .filter(|provider| provider.detected)
            .map(|provider| provider.id.clone())
            .collect::<HashSet<_>>();
        let previously_known = settings
            .known_provider_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let can_identify_new_providers = !fresh_install && !previously_known.is_empty();
        let auto_enable_provider_ids = registry
            .catalog()
            .providers
            .iter()
            .filter(|provider| {
                can_identify_new_providers && !previously_known.contains(&provider.id)
            })
            .map(|provider| provider.id.clone())
            .collect();

        normalize(&registry, &mut settings, &detected);
        storage.save_settings(&settings)?;
        let provider_ids = registry
            .catalog()
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect();
        let service = Self {
            storage,
            registry,
            settings: RwLock::new(settings),
            enablement_revision: AtomicU64::new(0),
        };
        let plan = CredentialDetectionPlan {
            provider_ids,
            auto_enable_provider_ids,
            replace_fallback: fresh_install,
            enablement_revision: 0,
        };
        Ok((service, plan))
    }

    pub fn get(&self) -> AppSettings {
        self.settings
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    pub fn update(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        let mut current = self
            .settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?;
        let enabled_before = enabled_provider_set(&current);
        let detected = current
            .providers
            .iter()
            .filter(|provider| provider.detected)
            .map(|provider| provider.id.clone())
            .collect::<HashSet<_>>();
        normalize(&self.registry, &mut settings, &detected);
        self.storage
            .save_settings(&settings)
            .map_err(|_| "OpenQuota settings could not be saved.".to_owned())?;
        let enablement_changed = enabled_provider_set(&settings) != enabled_before;
        current.clone_from(&settings);
        if enablement_changed {
            self.enablement_revision.fetch_add(1, Ordering::SeqCst);
        }
        Ok(settings)
    }

    pub fn reset_detection_plan(&self) -> CredentialDetectionPlan {
        CredentialDetectionPlan {
            provider_ids: self
                .registry
                .catalog()
                .providers
                .iter()
                .map(|provider| provider.id.clone())
                .collect(),
            auto_enable_provider_ids: HashSet::new(),
            replace_fallback: true,
            enablement_revision: self.enablement_revision.load(Ordering::SeqCst),
        }
    }

    /// Applies a completed local credential probe without overriding settings changed while it ran.
    pub fn apply_credential_detection(
        &self,
        plan: &CredentialDetectionPlan,
        detected: &HashSet<String>,
    ) -> Result<CredentialDetectionOutcome, String> {
        let mut current = self
            .settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?;
        let enabled_before = enabled_provider_set(&current);
        let mut next = current.clone();
        normalize(&self.registry, &mut next, detected);

        if plan.replace_fallback {
            if self.enablement_revision.load(Ordering::SeqCst) == plan.enablement_revision
                && !detected.is_empty()
            {
                for provider in &mut next.providers {
                    provider.enabled = detected.contains(&provider.id);
                }
            }
        } else {
            for provider in &mut next.providers {
                if plan.auto_enable_provider_ids.contains(&provider.id)
                    && detected.contains(&provider.id)
                {
                    provider.enabled = true;
                }
            }
        }

        self.storage
            .save_settings(&next)
            .map_err(|_| "OpenQuota settings could not be saved.".to_owned())?;
        let newly_enabled_provider_ids = next
            .providers
            .iter()
            .filter(|provider| provider.enabled && !enabled_before.contains(&provider.id))
            .map(|provider| provider.id.clone())
            .collect();
        let enablement_changed = enabled_provider_set(&next) != enabled_before;
        current.clone_from(&next);
        if enablement_changed {
            self.enablement_revision.fetch_add(1, Ordering::SeqCst);
        }
        Ok(CredentialDetectionOutcome {
            settings: next,
            newly_enabled_provider_ids,
        })
    }

    pub fn enabled_provider_ids(&self) -> Vec<String> {
        self.get()
            .providers
            .into_iter()
            .filter(|provider| provider.enabled)
            .map(|provider| provider.id)
            .collect()
    }

    pub fn reset_provider(&self, provider_id: &str) -> Result<AppSettings, String> {
        let mut settings = self.get();
        let definition = self
            .registry
            .definition(provider_id)
            .ok_or_else(|| "Unknown provider.".to_owned())?;
        let provider = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| "Provider settings are unavailable.".to_owned())?;
        provider.expanded = false;
        provider.metrics = default_provider(definition, provider.detected).metrics;
        self.update(settings)
    }

    pub fn default_settings(&self, detected: &HashSet<String>) -> AppSettings {
        default_settings(&self.registry, detected)
    }

    pub fn catalog(&self) -> &ProviderCatalog {
        self.registry.catalog()
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
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

fn enabled_provider_set(settings: &AppSettings) -> HashSet<String> {
    settings
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| provider.id.clone())
        .collect()
}

pub fn default_settings(registry: &ProviderRegistry, detected: &HashSet<String>) -> AppSettings {
    let catalog = registry.catalog();
    let mut settings = AppSettings {
        known_provider_ids: catalog
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect(),
        providers: catalog
            .providers
            .iter()
            .map(|provider| default_provider(provider, detected.contains(&provider.id)))
            .collect(),
        ..AppSettings::default()
    };
    if !settings.providers.iter().any(|provider| provider.enabled) {
        for provider in &mut settings.providers {
            provider.enabled = registry
                .definition(&provider.id)
                .is_some_and(|definition| definition.fallback_enabled);
        }
    }
    settings
}

pub fn normalize(
    registry: &ProviderRegistry,
    settings: &mut AppSettings,
    detected: &HashSet<String>,
) {
    let catalog = registry.catalog();
    let migrating_to_multi_provider = settings.schema_version < 3;
    settings.schema_version = 5;
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

    let mut normalized = Vec::new();
    for mut provider in settings.providers.clone() {
        let Some(definition) = registry.definition(&provider.id) else {
            continue;
        };
        if normalized
            .iter()
            .any(|known: &ProviderLayout| known.id == provider.id)
        {
            continue;
        }
        let was_known = settings
            .known_provider_ids
            .iter()
            .any(|known| known == &definition.id);
        if !was_known {
            provider.enabled = detected.contains(&definition.id);
            settings.known_provider_ids.push(definition.id.clone());
        }
        provider.detected = detected.contains(&definition.id);
        normalize_metrics(&mut provider.metrics, &definition.metrics);
        normalized.push(provider);
    }
    for definition in &catalog.providers {
        if normalized
            .iter()
            .any(|provider| provider.id == definition.id)
        {
            continue;
        }
        let was_known = settings
            .known_provider_ids
            .iter()
            .any(|known| known == &definition.id);
        let is_detected = detected.contains(&definition.id);
        let mut provider = default_provider(definition, is_detected);
        provider.enabled = !was_known && is_detected;
        settings.known_provider_ids.push(definition.id.clone());
        normalized.push(provider);
    }
    if migrating_to_multi_provider {
        normalized.sort_by_key(|provider| {
            catalog
                .providers
                .iter()
                .position(|definition| definition.id == provider.id)
                .unwrap_or(usize::MAX)
        });
    }
    settings.providers = normalized;
    settings.known_provider_ids.sort();
    settings.known_provider_ids.dedup();
}

fn default_provider(definition: &ProviderDefinition, detected: bool) -> ProviderLayout {
    ProviderLayout {
        id: definition.id.clone(),
        enabled: detected,
        detected,
        expanded: false,
        metrics: definition
            .metrics
            .iter()
            .map(|metric| MetricLayout {
                id: metric.id.clone(),
                enabled: metric.default_enabled,
                section: metric.default_section,
                pinned: metric.default_pinned,
            })
            .collect(),
    }
}

fn normalize_metrics(metrics: &mut Vec<MetricLayout>, definitions: &[MetricDefinition]) {
    let mut normalized = Vec::with_capacity(definitions.len());
    for metric in metrics.iter() {
        if definitions
            .iter()
            .any(|definition| definition.id == metric.id)
            && !normalized
                .iter()
                .any(|known: &MetricLayout| known.id == metric.id)
        {
            normalized.push(metric.clone());
        }
    }
    for definition in definitions {
        if !normalized.iter().any(|metric| metric.id == definition.id) {
            normalized.push(MetricLayout {
                id: definition.id.clone(),
                enabled: definition.default_enabled,
                section: definition.default_section,
                pinned: definition.default_pinned,
            });
        }
    }
    let mut pin_count = 0;
    for metric in &mut normalized {
        let pinnable = definitions
            .iter()
            .find(|definition| definition.id == metric.id)
            .is_some_and(|definition| definition.pinnable);
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

    use crate::{
        models::{MetricSection, ProviderDefinition, ProviderSnapshot},
        providers::{
            antigravity, claude, codex, cursor, ProviderError, ProviderRegistry, UsageProvider,
        },
        storage::Storage,
    };

    use super::{default_settings, normalize, SettingsService, MAX_PINS_PER_PROVIDER};

    struct CatalogProvider(ProviderDefinition);

    impl UsageProvider for CatalogProvider {
        fn definition(&self) -> ProviderDefinition {
            self.0.clone()
        }

        fn has_local_credentials(&self) -> bool {
            false
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            unreachable!()
        }
    }

    fn catalog() -> Arc<ProviderRegistry> {
        let providers = [
            claude::definition(),
            codex::definition(),
            cursor::definition(),
            antigravity::definition(),
        ]
        .into_iter()
        .map(|definition| Arc::new(CatalogProvider(definition)) as Arc<dyn UsageProvider>)
        .collect();
        Arc::new(ProviderRegistry::new(providers).unwrap())
    }

    fn enabled_ids(settings: &crate::models::AppSettings) -> Vec<&str> {
        settings
            .providers
            .iter()
            .filter(|provider| provider.enabled)
            .map(|provider| provider.id.as_str())
            .collect()
    }

    #[test]
    fn empty_detection_uses_the_established_fallback_set() {
        let registry = catalog();
        let settings = default_settings(&registry, &HashSet::new());

        assert_eq!(enabled_ids(&settings), ["claude", "codex", "cursor"]);
    }

    #[test]
    fn deferred_first_run_replaces_fallback_with_detected_providers() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let (service, plan) = SettingsService::new_deferred(storage, catalog()).unwrap();
        assert_eq!(enabled_ids(&service.get()), ["claude", "codex", "cursor"]);

        let outcome = service
            .apply_credential_detection(&plan, &HashSet::from(["antigravity".to_owned()]))
            .unwrap();

        assert_eq!(enabled_ids(&outcome.settings), ["antigravity"]);
        assert_eq!(outcome.newly_enabled_provider_ids, ["antigravity"]);
        assert!(
            outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .detected
        );
    }

    #[test]
    fn deferred_first_run_keeps_fallback_when_nothing_is_detected() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let (service, plan) = SettingsService::new_deferred(storage, catalog()).unwrap();

        let outcome = service
            .apply_credential_detection(&plan, &HashSet::new())
            .unwrap();

        assert_eq!(
            enabled_ids(&outcome.settings),
            ["claude", "codex", "cursor"]
        );
        assert!(outcome.newly_enabled_provider_ids.is_empty());
    }

    #[test]
    fn user_enablement_change_wins_over_a_running_detection_pass() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let (service, plan) = SettingsService::new_deferred(storage, catalog()).unwrap();
        let mut changed = service.get();
        changed
            .providers
            .iter_mut()
            .find(|provider| provider.id == "claude")
            .unwrap()
            .enabled = false;
        service.update(changed).unwrap();

        let outcome = service
            .apply_credential_detection(&plan, &HashSet::from(["antigravity".to_owned()]))
            .unwrap();

        assert_eq!(enabled_ids(&outcome.settings), ["codex", "cursor"]);
        assert!(
            outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .detected
        );
    }

    #[test]
    fn deferred_new_provider_is_auto_enabled_only_once() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let registry = catalog();
        let mut saved = default_settings(&registry, &HashSet::from(["codex".to_owned()]));
        saved.known_provider_ids.retain(|id| id != "antigravity");
        saved
            .providers
            .retain(|provider| provider.id != "antigravity");
        storage.save_settings(&saved).unwrap();

        let (first, plan) =
            SettingsService::new_deferred(storage.clone(), registry.clone()).unwrap();
        let detected = HashSet::from(["codex".to_owned(), "antigravity".to_owned()]);
        let outcome = first.apply_credential_detection(&plan, &detected).unwrap();
        assert!(
            outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .enabled
        );
        let mut disabled = outcome.settings;
        disabled
            .providers
            .iter_mut()
            .find(|provider| provider.id == "antigravity")
            .unwrap()
            .enabled = false;
        first.update(disabled).unwrap();
        drop(first);

        let (second, second_plan) = SettingsService::new_deferred(storage, registry).unwrap();
        let outcome = second
            .apply_credential_detection(&second_plan, &detected)
            .unwrap();
        assert!(
            !outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn unrelated_toggle_does_not_cancel_new_provider_detection() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let registry = catalog();
        let mut saved = default_settings(&registry, &HashSet::from(["claude".to_owned()]));
        saved.known_provider_ids.retain(|id| id != "antigravity");
        saved
            .providers
            .retain(|provider| provider.id != "antigravity");
        storage.save_settings(&saved).unwrap();
        let (service, plan) = SettingsService::new_deferred(storage, registry).unwrap();
        let mut changed = service.get();
        changed
            .providers
            .iter_mut()
            .find(|provider| provider.id == "claude")
            .unwrap()
            .enabled = false;
        service.update(changed).unwrap();

        let outcome = service
            .apply_credential_detection(&plan, &HashSet::from(["antigravity".to_owned()]))
            .unwrap();

        assert!(
            outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "antigravity")
                .unwrap()
                .enabled
        );
        assert!(
            !outcome
                .settings
                .providers
                .iter()
                .find(|provider| provider.id == "claude")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn normalization_preserves_order_and_enforces_pin_cap_per_provider() {
        let detected = HashSet::from(["codex".to_owned(), "claude".to_owned()]);
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &detected);
        let metrics = &mut settings.providers[0].metrics;
        metrics.rotate_left(2);
        for metric in metrics.iter_mut() {
            metric.enabled = true;
            metric.pinned = true;
        }
        normalize(&catalog, &mut settings, &detected);
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
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &detected);
        for metric in &mut settings.providers[1].metrics {
            metric.section = MetricSection::OnDemand;
        }
        normalize(&catalog, &mut settings, &detected);
        assert!(settings.providers[1]
            .metrics
            .iter()
            .any(|metric| metric.enabled && metric.section == MetricSection::AlwaysVisible));
    }

    #[test]
    fn normalization_adds_new_codex_metrics_without_disturbing_existing_order() {
        let detected = HashSet::from(["codex".to_owned()]);
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &detected);
        let codex = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "codex")
            .unwrap();
        codex.metrics.retain(|metric| {
            !matches!(
                metric.id.as_str(),
                "codex.spark" | "codex.sparkWeekly" | "codex.credits" | "codex.rateLimitResets"
            )
        });

        normalize(&catalog, &mut settings, &detected);

        let codex = settings
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .unwrap();
        assert_eq!(
            &codex.metrics[..2]
                .iter()
                .map(|metric| metric.id.as_str())
                .collect::<Vec<_>>(),
            &["codex.session", "codex.weekly"]
        );
        for id in [
            "codex.spark",
            "codex.sparkWeekly",
            "codex.credits",
            "codex.rateLimitResets",
        ] {
            let metric = codex.metrics.iter().find(|metric| metric.id == id).unwrap();
            assert!(metric.enabled);
            assert_eq!(metric.section, MetricSection::OnDemand);
            assert!(!metric.pinned);
        }
    }

    #[test]
    fn layout_and_preferences_survive_a_service_restart() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let detected = HashSet::from(["codex".to_owned(), "antigravity".to_owned()]);
        let catalog = catalog();
        let first =
            SettingsService::new_for_test(storage.clone(), catalog.clone(), &detected).unwrap();
        let mut settings = first.get();
        settings.density = crate::models::DensityPreference::Compact;
        settings.dismissed_update_version = Some("0.2.0".to_owned());
        settings.last_update_check_at = Some(chrono::Utc::now());
        settings.providers.rotate_left(1);
        settings.providers[1].metrics.rotate_right(1);
        let expected = first.update(settings).unwrap();
        let second = SettingsService::new_for_test(storage, catalog, &detected).unwrap();
        assert_eq!(second.get(), expected);
    }

    #[test]
    fn new_detected_provider_is_enabled_once_without_overriding_later_choice() {
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &HashSet::from(["codex".to_owned()]));
        settings.known_provider_ids.retain(|id| id != "antigravity");
        settings
            .providers
            .retain(|provider| provider.id != "antigravity");
        let detected = HashSet::from(["codex".to_owned(), "antigravity".to_owned()]);
        normalize(&catalog, &mut settings, &detected);
        let antigravity = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "antigravity")
            .unwrap();
        assert!(antigravity.enabled);
        antigravity.enabled = false;
        normalize(&catalog, &mut settings, &detected);
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
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &HashSet::from(["codex".to_owned()]));
        settings.schema_version = 2;
        settings.known_provider_ids.clear();
        settings.providers.retain(|provider| provider.id == "codex");
        normalize(
            &catalog,
            &mut settings,
            &HashSet::from(["codex".to_owned(), "antigravity".to_owned()]),
        );
        assert_eq!(settings.schema_version, 5);
        assert_eq!(
            settings
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex", "cursor", "antigravity"]
        );
    }

    #[test]
    fn invalid_saved_settings_are_not_overwritten_with_defaults() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("openquota.db");
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE app_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    payload TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO app_settings(id, payload) VALUES (1, ?1)",
                ["{not-valid-json"],
            )
            .unwrap();
        drop(connection);
        let storage = Arc::new(Storage::open(&database_path).unwrap());

        assert!(SettingsService::new_deferred(storage.clone(), catalog()).is_err());
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        let payload: String = connection
            .query_row("SELECT payload FROM app_settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(payload, "{not-valid-json");
    }

    #[test]
    fn provider_reset_uses_the_backend_catalog_and_preserves_provider_state() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let detected = HashSet::from(["codex".to_owned()]);
        let catalog = catalog();
        let service = SettingsService::new_for_test(storage, catalog.clone(), &detected).unwrap();
        let mut settings = service.get();
        let codex = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "codex")
            .unwrap();
        codex.enabled = false;
        codex.expanded = true;
        codex.metrics.reverse();
        codex.metrics[0].pinned = true;
        service.update(settings).unwrap();

        let reset = service.reset_provider("codex").unwrap();
        let codex = reset
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .unwrap();
        let defaults = default_settings(&catalog, &detected);
        let default_codex = defaults
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .unwrap();

        assert!(!codex.enabled);
        assert!(codex.detected);
        assert!(!codex.expanded);
        assert_eq!(codex.metrics, default_codex.metrics);
    }
}
