use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use serde_json::{Map, Value};

use crate::{
    hashing::sha256_hex,
    models::{
        AppSettings, MetricDefinition, MetricLayout, MetricSection, ProviderCatalog,
        ProviderDefinition, ProviderLayout, SettingsViewState,
    },
    providers::{CredentialProbeResults, CredentialProbeStatus, ProviderRegistry},
    storage::{ProviderAccountUpdate, Storage, StorageError},
};

pub const MAX_PINS_PER_PROVIDER: usize = 2;

#[derive(Debug, Clone)]
pub struct CredentialDetectionPlan {
    provider_ids: Vec<String>,
    auto_enable_provider_ids: HashSet<String>,
    replace_fallback: bool,
    enablement_revision: u64,
    credential_revision: u64,
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
    command_mutation: tokio::sync::Mutex<()>,
    credential_mutation: tokio::sync::Mutex<()>,
    enablement_revision: AtomicU64,
    credential_revision: AtomicU64,
    settings_revision: AtomicU64,
    account_revision: AtomicU64,
    active_account_identities: RwLock<HashMap<String, String>>,
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
        let persisted_accounts = persisted_account_provider_ids(&storage)?;
        normalize_with_persisted_accounts(&registry, &mut settings, detected, &persisted_accounts);
        storage.save_settings(&settings)?;
        let service = Self {
            storage,
            registry,
            settings: RwLock::new(settings),
            command_mutation: tokio::sync::Mutex::new(()),
            credential_mutation: tokio::sync::Mutex::new(()),
            enablement_revision: AtomicU64::new(0),
            credential_revision: AtomicU64::new(0),
            settings_revision: AtomicU64::new(0),
            account_revision: AtomicU64::new(0),
            active_account_identities: RwLock::new(HashMap::new()),
        };
        service.activate_launch_accounts()?;
        Ok(service)
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
            .filter(|provider| provider.detected && registry.definition(&provider.id).is_some())
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

        let persisted_accounts = persisted_account_provider_ids(&storage)?;
        normalize_with_persisted_accounts(&registry, &mut settings, &detected, &persisted_accounts);
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
            command_mutation: tokio::sync::Mutex::new(()),
            credential_mutation: tokio::sync::Mutex::new(()),
            enablement_revision: AtomicU64::new(0),
            credential_revision: AtomicU64::new(0),
            settings_revision: AtomicU64::new(0),
            account_revision: AtomicU64::new(0),
            active_account_identities: RwLock::new(HashMap::new()),
        };
        service.activate_launch_accounts()?;
        let plan = CredentialDetectionPlan {
            provider_ids,
            auto_enable_provider_ids,
            replace_fallback: fresh_install,
            enablement_revision: 0,
            credential_revision: 0,
        };
        Ok((service, plan))
    }

    pub fn get(&self) -> AppSettings {
        self.settings
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn lock_command_mutation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.command_mutation.lock().await
    }

    pub(crate) async fn lock_credential_mutation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.credential_mutation.lock().await
    }

    fn get_with_revisions(&self) -> (AppSettings, u64, u64) {
        self.settings
            .read()
            .map(|settings| {
                (
                    settings.clone(),
                    self.settings_revision.load(Ordering::SeqCst),
                    self.account_revision.load(Ordering::SeqCst),
                )
            })
            .unwrap_or_else(|_| {
                (
                    AppSettings::default(),
                    self.settings_revision(),
                    self.account_revision(),
                )
            })
    }

    fn activate_launch_accounts(&self) -> Result<(), StorageError> {
        let identity = self
            .registry
            .cache_identity("codex")
            .resolved_value()
            .map(str::to_owned);
        if let Some(identity) = identity {
            self.activate_account("codex", "codex", &identity)?;
        }
        Ok(())
    }

    pub fn activate_account(
        &self,
        family: &str,
        provider_id: &str,
        identity: &str,
    ) -> Result<bool, StorageError> {
        let mut settings = self.settings.write().map_err(|_| StorageError::Poisoned)?;
        let mut active_accounts = self
            .active_account_identities
            .write()
            .map_err(|_| StorageError::Poisoned)?;
        let previous_identity = active_accounts.get(provider_id).cloned();
        if previous_identity.as_deref() == Some(identity) {
            return Ok(false);
        }

        let records = self.storage.load_provider_account_records(family)?;
        let (record_id, mut payload) = account_record(&records, family, identity);
        let current_name = settings.provider_names.get(provider_id).map(String::as_str);
        let mut account_updates = Vec::new();

        // Before account-aware names existed, the visible card held the only saved name. Preserve
        // that name on the original bare-id account before projecting a different active account.
        if previous_identity.is_none() {
            if let Some((legacy_identity, _, legacy_payload)) = records
                .iter()
                .find(|(_, record_provider_id, _)| record_provider_id == provider_id)
            {
                let mut legacy_payload = serde_json::from_str(legacy_payload)
                    .unwrap_or_else(|_| Value::Object(Map::new()));
                if legacy_identity != identity
                    && account_custom_name(&legacy_payload).is_none()
                    && current_name.is_some()
                {
                    set_account_custom_name(&mut legacy_payload, current_name);
                    account_updates.push(ProviderAccountUpdate {
                        provider_family: family.to_owned(),
                        identity_key: legacy_identity.clone(),
                        provider_id: provider_id.to_owned(),
                        payload: serde_json::to_string(&legacy_payload)?,
                    });
                }
            }
        }
        if account_custom_name(&payload).is_none()
            && record_id == provider_id
            && previous_identity.is_none()
        {
            set_account_custom_name(&mut payload, current_name);
        }

        let mut next = settings.clone();
        match account_custom_name(&payload) {
            Some(name) => {
                next.provider_names.insert(provider_id.to_owned(), name);
            }
            None => {
                next.provider_names.remove(provider_id);
            }
        }

        account_updates.push(ProviderAccountUpdate {
            provider_family: family.to_owned(),
            identity_key: identity.to_owned(),
            provider_id: record_id,
            payload: serde_json::to_string(&payload)?,
        });
        self.storage
            .save_settings_with_account_updates(&next, &account_updates)?;
        settings.clone_from(&next);
        active_accounts.insert(provider_id.to_owned(), identity.to_owned());
        self.settings_revision.fetch_add(1, Ordering::SeqCst);
        self.account_revision.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    pub fn settings_revision(&self) -> u64 {
        self.settings_revision.load(Ordering::SeqCst)
    }

    pub fn account_revision(&self) -> u64 {
        self.account_revision.load(Ordering::SeqCst)
    }

    fn active_account_name_updates(
        &self,
        settings: &AppSettings,
    ) -> Result<Vec<ProviderAccountUpdate>, StorageError> {
        let active = self
            .active_account_identities
            .read()
            .map_err(|_| StorageError::Poisoned)?
            .clone();
        let mut updates = Vec::new();
        for (provider_id, identity) in active {
            let family = crate::providers::provider_family(&provider_id);
            let records = self.storage.load_provider_account_records(family)?;
            let (record_id, mut payload) = account_record(&records, family, &identity);
            set_account_custom_name(
                &mut payload,
                settings
                    .provider_names
                    .get(&provider_id)
                    .map(String::as_str),
            );
            updates.push(ProviderAccountUpdate {
                provider_family: family.to_owned(),
                identity_key: identity,
                provider_id: record_id,
                payload: serde_json::to_string(&payload)?,
            });
        }
        Ok(updates)
    }

    #[cfg(test)]
    pub fn update(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        self.update_internal(&mut settings, None, None, false)
    }

    pub fn update_from_view(
        &self,
        mut settings: AppSettings,
        expected_settings_revision: u64,
        expected_account_revision: u64,
    ) -> Result<AppSettings, String> {
        self.update_internal(
            &mut settings,
            Some(expected_settings_revision),
            Some(expected_account_revision),
            false,
        )
    }

    pub fn reset_all_from_view(
        &self,
        mut settings: AppSettings,
        expected_settings_revision: u64,
        expected_account_revision: u64,
    ) -> Result<AppSettings, String> {
        self.update_internal(
            &mut settings,
            Some(expected_settings_revision),
            Some(expected_account_revision),
            true,
        )
    }

    fn update_internal(
        &self,
        settings: &mut AppSettings,
        expected_settings_revision: Option<u64>,
        expected_account_revision: Option<u64>,
        reset_all_account_names: bool,
    ) -> Result<AppSettings, String> {
        let mut current = self
            .settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?;
        if expected_settings_revision
            .is_some_and(|revision| revision != self.settings_revision.load(Ordering::SeqCst))
        {
            return Err(
                "Settings changed before they could be saved. Please try again.".to_owned(),
            );
        }
        let enabled_before = enabled_provider_set(&current);
        let detected = current
            .providers
            .iter()
            .filter(|provider| provider.detected)
            .map(|provider| provider.id.clone())
            .collect::<HashSet<_>>();
        let persisted_accounts = persisted_account_provider_ids(&self.storage)
            .map_err(|_| "OpenQuota account settings could not be loaded.".to_owned())?;
        normalize_with_persisted_accounts(&self.registry, settings, &detected, &persisted_accounts);
        if expected_account_revision != Some(self.account_revision.load(Ordering::SeqCst)) {
            let active_provider_ids = self
                .active_account_identities
                .read()
                .map_err(|_| "OpenQuota account names are temporarily unavailable.".to_owned())?
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            for provider_id in active_provider_ids {
                if let Some(name) = current.provider_names.get(&provider_id) {
                    settings.provider_names.insert(provider_id, name.clone());
                } else {
                    settings.provider_names.remove(&provider_id);
                }
            }
        }
        let account_updates = if reset_all_account_names {
            self.account_name_reset_updates()
        } else {
            self.active_account_name_updates(settings)
        }
        .map_err(|_| "OpenQuota account names could not be saved.".to_owned())?;
        self.storage
            .save_settings_with_account_updates(settings, &account_updates)
            .map_err(|_| "OpenQuota settings could not be saved.".to_owned())?;
        let enablement_changed = enabled_provider_set(settings) != enabled_before;
        current.clone_from(settings);
        if enablement_changed {
            self.enablement_revision.fetch_add(1, Ordering::SeqCst);
        }
        self.settings_revision.fetch_add(1, Ordering::SeqCst);
        Ok(settings.clone())
    }

    fn account_name_reset_updates(&self) -> Result<Vec<ProviderAccountUpdate>, StorageError> {
        self.storage
            .load_all_provider_account_records()?
            .into_iter()
            .map(|(provider_family, identity_key, provider_id, payload)| {
                let mut payload = serde_json::from_str::<Value>(&payload)?;
                if let Some(object) = payload.as_object_mut() {
                    object.remove("customName");
                }
                Ok(ProviderAccountUpdate {
                    provider_family,
                    identity_key,
                    provider_id,
                    payload: serde_json::to_string(&payload)?,
                })
            })
            .collect()
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
            credential_revision: self.credential_revision.load(Ordering::SeqCst),
        }
    }

    /// Applies a completed local credential probe without overriding settings changed while it ran.
    pub fn apply_credential_detection(
        &self,
        plan: &CredentialDetectionPlan,
        probe_results: &CredentialProbeResults,
    ) -> Result<CredentialDetectionOutcome, String> {
        let mut current = self
            .settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?;
        let enabled_before = enabled_provider_set(&current);
        let detected_before = detected_provider_set(&current);
        let credential_revision_matches =
            self.credential_revision.load(Ordering::SeqCst) == plan.credential_revision;
        let mut next = current.clone();
        let mut detected = detected_before.clone();
        if credential_revision_matches {
            for (provider_id, status) in probe_results {
                match status {
                    CredentialProbeStatus::Detected => {
                        detected.insert(provider_id.clone());
                    }
                    CredentialProbeStatus::Absent => {
                        detected.remove(provider_id);
                    }
                    CredentialProbeStatus::Unknown => {}
                }
            }
        }
        let persisted_accounts = persisted_account_provider_ids(&self.storage)
            .map_err(|_| "OpenQuota account settings could not be loaded.".to_owned())?;
        normalize_with_persisted_accounts(
            &self.registry,
            &mut next,
            &detected,
            &persisted_accounts,
        );

        if plan.replace_fallback {
            if credential_revision_matches
                && self.enablement_revision.load(Ordering::SeqCst) == plan.enablement_revision
            {
                let any_detected = !detected.is_empty();
                for provider in &mut next.providers {
                    match probe_results.get(&provider.id) {
                        Some(CredentialProbeStatus::Detected) => provider.enabled = true,
                        Some(CredentialProbeStatus::Absent) => {
                            provider.enabled = !any_detected
                                && self
                                    .registry
                                    .definition(&provider.id)
                                    .is_some_and(|definition| definition.fallback_enabled);
                        }
                        Some(CredentialProbeStatus::Unknown) | None => {}
                    }
                }
            }
        } else if credential_revision_matches {
            for provider in &mut next.providers {
                if plan.auto_enable_provider_ids.contains(&provider.id)
                    && probe_results.get(&provider.id) == Some(&CredentialProbeStatus::Detected)
                {
                    provider.enabled = true;
                }
            }
        }

        if next == *current {
            return Ok(CredentialDetectionOutcome {
                settings: next,
                newly_enabled_provider_ids: Vec::new(),
            });
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
        if detected_provider_set(&next) != detected_before {
            self.credential_revision.fetch_add(1, Ordering::SeqCst);
        }
        self.settings_revision.fetch_add(1, Ordering::SeqCst);
        Ok(CredentialDetectionOutcome {
            settings: next,
            newly_enabled_provider_ids,
        })
    }

    pub fn enabled_provider_ids(&self) -> Vec<String> {
        self.get()
            .providers
            .into_iter()
            .filter(|provider| provider.enabled && self.registry.definition(&provider.id).is_some())
            .map(|provider| provider.id)
            .collect()
    }

    pub fn reset_provider(
        &self,
        provider_id: &str,
        expected_settings_revision: u64,
        expected_account_revision: u64,
    ) -> Result<AppSettings, String> {
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
        self.update_from_view(
            settings,
            expected_settings_revision,
            expected_account_revision,
        )
    }

    pub fn reset_defaults(&self) -> AppSettings {
        self.default_settings(&detected_provider_set(&self.get()))
    }

    pub fn record_provider_credential_mutation(&self) {
        self.credential_revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn reconcile_provider_credential_state(
        &self,
        provider_id: &str,
        detected: bool,
        enable: bool,
    ) -> Result<AppSettings, String> {
        let mut current = self
            .settings
            .write()
            .map_err(|_| "OpenQuota settings are temporarily unavailable.".to_owned())?;
        let enabled_before = enabled_provider_set(&current);
        let mut next = current.clone();
        let provider = next
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| "Provider settings are unavailable.".to_owned())?;
        provider.detected = detected;
        if enable {
            provider.enabled = true;
        }
        self.storage
            .save_settings(&next)
            .map_err(|_| "OpenQuota settings could not be saved.".to_owned())?;
        current.clone_from(&next);
        if enabled_provider_set(&next) != enabled_before {
            self.enablement_revision.fetch_add(1, Ordering::SeqCst);
        }
        self.settings_revision.fetch_add(1, Ordering::SeqCst);
        Ok(next)
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
        tray_available: bool,
        platform_summary: Option<String>,
    ) -> SettingsViewState {
        let (settings, settings_revision, account_revision) = self.get_with_revisions();
        let mut renamable_provider_ids = self.registry.observed_account_provider_ids();
        if let Ok(stored_ids) = self.storage.load_observed_account_provider_ids() {
            for provider_id in stored_ids {
                if self.registry.supports_account_names(&provider_id)
                    && !renamable_provider_ids.contains(&provider_id)
                {
                    renamable_provider_ids.push(provider_id);
                }
            }
        }
        SettingsViewState {
            settings,
            settings_revision,
            account_revision,
            renamable_provider_ids,
            notification_permission: notification_permission.into(),
            integration_error,
            tray_available,
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

fn detected_provider_set(settings: &AppSettings) -> HashSet<String> {
    settings
        .providers
        .iter()
        .filter(|provider| provider.detected)
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

#[cfg(test)]
pub fn normalize(
    registry: &ProviderRegistry,
    settings: &mut AppSettings,
    detected: &HashSet<String>,
) {
    normalize_with_persisted_accounts(registry, settings, detected, &HashSet::new());
}

fn normalize_with_persisted_accounts(
    registry: &ProviderRegistry,
    settings: &mut AppSettings,
    detected: &HashSet<String>,
    persisted_accounts: &HashSet<String>,
) {
    let catalog = registry.catalog();
    let migrating_to_multi_provider = settings.schema_version < 3;
    settings.schema_version = 7;
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
    settings.provider_names.retain(|provider_id, name| {
        if !registry.supports_account_names(provider_id)
            && !persisted_accounts.contains(provider_id)
        {
            return false;
        }
        *name = name.trim().chars().take(48).collect();
        !name.is_empty()
    });

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
            if persisted_accounts.contains(&provider.id) {
                provider.detected = false;
                normalized.push(provider);
            }
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
        if crate::providers::is_claude_account_provider_id(&provider.id)
            || crate::providers::is_codex_account_provider_id(&provider.id)
        {
            let family = crate::providers::provider_family(&provider.id);
            let index = normalized
                .iter()
                .rposition(|known: &ProviderLayout| {
                    crate::providers::provider_family(&known.id) == family
                })
                .map_or(normalized.len(), |index| index + 1);
            normalized.insert(index, provider);
        } else {
            normalized.push(provider);
        }
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

fn persisted_account_provider_ids(storage: &Storage) -> Result<HashSet<String>, StorageError> {
    Ok(storage
        .load_observed_account_provider_ids()?
        .into_iter()
        .collect())
}

fn account_record(
    records: &[(String, String, String)],
    family: &str,
    identity: &str,
) -> (String, Value) {
    if let Some((_, provider_id, payload)) = records
        .iter()
        .find(|(known_identity, _, _)| known_identity == identity)
    {
        return (
            provider_id.clone(),
            serde_json::from_str(payload).unwrap_or_else(|_| Value::Object(Map::new())),
        );
    }

    let provider_id = if records.is_empty() {
        family.to_owned()
    } else {
        allocate_account_provider_id(records, family, identity)
    };
    (provider_id, Value::Object(Map::new()))
}

fn allocate_account_provider_id(
    records: &[(String, String, String)],
    family: &str,
    identity: &str,
) -> String {
    let occupied = records
        .iter()
        .map(|(_, provider_id, _)| provider_id.as_str())
        .collect::<HashSet<_>>();
    for salt in 0_u32.. {
        let suffix = if salt == 0
            && identity.len() >= 8
            && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            identity[..8].to_ascii_lowercase()
        } else {
            sha256_hex(format!("{identity}:{salt}").as_bytes())[..8].to_owned()
        };
        let candidate = format!("{family}@{suffix}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!()
}

fn account_custom_name(payload: &Value) -> Option<String> {
    payload
        .get("customName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn set_account_custom_name(payload: &mut Value, name: Option<&str>) {
    if !payload.is_object() {
        *payload = Value::Object(Map::new());
    }
    let object = payload
        .as_object_mut()
        .expect("account payload was initialized as an object");
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    if let Some(name) = name {
        object.insert("customName".into(), Value::String(name.to_owned()));
    } else {
        object.remove("customName");
    }
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
        metric.pinned &= pinnable && pin_count < MAX_PINS_PER_PROVIDER;
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
    use std::{
        collections::HashSet,
        sync::{atomic::Ordering, Arc},
    };

    use serde_json::Value;
    use tempfile::tempdir;

    use crate::{
        models::{MetricSection, ProviderDefinition, ProviderSnapshot, ThemePreference},
        providers::{
            antigravity, claude, codex, cursor, openrouter, CredentialProbeResults,
            CredentialProbeStatus, ProviderError, ProviderRegistry, UsageProvider,
        },
        storage::Storage,
    };

    use super::{
        default_settings, normalize, normalize_with_persisted_accounts, SettingsService,
        MAX_PINS_PER_PROVIDER,
    };

    struct CatalogProvider(ProviderDefinition);

    impl UsageProvider for CatalogProvider {
        fn definition(&self) -> ProviderDefinition {
            self.0.clone()
        }

        fn supports_account_names(&self) -> bool {
            matches!(
                crate::providers::provider_family(&self.0.id),
                "claude" | "codex"
            )
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
            openrouter::definition(),
        ]
        .into_iter()
        .map(|definition| Arc::new(CatalogProvider(definition)) as Arc<dyn UsageProvider>)
        .collect();
        Arc::new(ProviderRegistry::new(providers).unwrap())
    }

    fn claude_account_definition() -> ProviderDefinition {
        let mut definition = claude::definition();
        definition.id = "claude@1234abcd".into();
        definition.display_name = "Claude — Work".into();
        definition.fallback_enabled = false;
        for metric in &mut definition.metrics {
            metric.id = metric.id.replacen("claude.", "claude@1234abcd.", 1);
        }
        definition
    }

    fn catalog_with_claude_account() -> Arc<ProviderRegistry> {
        let providers = [
            claude::definition(),
            claude_account_definition(),
            codex::definition(),
            cursor::definition(),
            antigravity::definition(),
            openrouter::definition(),
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

    fn probe_results(detected: &[&str]) -> CredentialProbeResults {
        ["claude", "codex", "cursor", "antigravity", "openrouter"]
            .into_iter()
            .map(|provider_id| {
                let status = if detected.contains(&provider_id) {
                    CredentialProbeStatus::Detected
                } else {
                    CredentialProbeStatus::Absent
                };
                (provider_id.to_owned(), status)
            })
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
            .apply_credential_detection(&plan, &probe_results(&["antigravity"]))
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
            .apply_credential_detection(&plan, &probe_results(&[]))
            .unwrap();

        assert_eq!(
            enabled_ids(&outcome.settings),
            ["claude", "codex", "cursor"]
        );
        assert!(outcome.newly_enabled_provider_ids.is_empty());
    }

    #[test]
    fn unknown_first_run_probes_preserve_the_fallback_enablement() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let (service, plan) = SettingsService::new_deferred(storage, catalog()).unwrap();
        let unknown = ["claude", "codex", "cursor", "antigravity", "openrouter"]
            .into_iter()
            .map(|provider_id| (provider_id.to_owned(), CredentialProbeStatus::Unknown))
            .collect();

        let outcome = service.apply_credential_detection(&plan, &unknown).unwrap();

        assert_eq!(
            enabled_ids(&outcome.settings),
            ["claude", "codex", "cursor"]
        );
        assert!(outcome
            .settings
            .providers
            .iter()
            .all(|provider| !provider.detected));
    }

    #[test]
    fn unknown_reset_probe_preserves_existing_detection_and_enablement() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service =
            SettingsService::new_for_test(storage, catalog(), &HashSet::from(["codex".to_owned()]))
                .unwrap();
        let plan = service.reset_detection_plan();
        let mut results = probe_results(&["antigravity"]);
        results.insert("codex".to_owned(), CredentialProbeStatus::Unknown);

        let outcome = service.apply_credential_detection(&plan, &results).unwrap();
        let codex = outcome
            .settings
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .unwrap();
        let antigravity = outcome
            .settings
            .providers
            .iter()
            .find(|provider| provider.id == "antigravity")
            .unwrap();

        assert!(codex.detected);
        assert!(codex.enabled);
        assert!(antigravity.detected);
        assert!(antigravity.enabled);
    }

    #[test]
    fn definitive_reset_absence_restores_the_fallback_set() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(
            storage,
            catalog(),
            &HashSet::from(["antigravity".to_owned()]),
        )
        .unwrap();
        let plan = service.reset_detection_plan();

        let outcome = service
            .apply_credential_detection(&plan, &probe_results(&[]))
            .unwrap();

        assert_eq!(
            enabled_ids(&outcome.settings),
            ["claude", "codex", "cursor"]
        );
        assert!(outcome
            .settings
            .providers
            .iter()
            .all(|provider| !provider.detected));
    }

    #[test]
    fn unknown_existing_detection_does_not_enable_the_fallback_set() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service =
            SettingsService::new_for_test(storage, catalog(), &HashSet::from(["codex".to_owned()]))
                .unwrap();
        let plan = service.reset_detection_plan();
        let mut results = probe_results(&[]);
        results.insert("codex".to_owned(), CredentialProbeStatus::Unknown);

        let outcome = service.apply_credential_detection(&plan, &results).unwrap();

        assert_eq!(enabled_ids(&outcome.settings), ["codex"]);
        assert!(outcome
            .settings
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .is_some_and(|provider| provider.detected));
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
            .apply_credential_detection(&plan, &probe_results(&["antigravity"]))
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
        let detected = probe_results(&["codex", "antigravity"]);
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
    fn account_name_and_rename_availability_survive_a_restart() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        storage
            .save_provider_account_record("claude", "identity-a", "claude", "{}")
            .unwrap();
        let registry = catalog();
        let (first, _) = SettingsService::new_deferred(storage.clone(), registry.clone()).unwrap();
        let mut settings = first.get();
        settings
            .provider_names
            .insert("claude".into(), "Personal".into());
        first
            .update_from_view(
                settings,
                first.settings_revision(),
                first.account_revision(),
            )
            .unwrap();
        drop(first);

        let (second, _) = SettingsService::new_deferred(storage, registry).unwrap();
        let state = second.view_state("prompt", None, false, None);

        assert_eq!(
            state
                .settings
                .provider_names
                .get("claude")
                .map(String::as_str),
            Some("Personal")
        );
        assert!(state.renamable_provider_ids.contains(&"claude".to_owned()));
    }

    #[test]
    fn codex_names_follow_accounts_when_the_active_login_changes() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(
            storage.clone(),
            catalog(),
            &HashSet::from(["codex".to_owned()]),
        )
        .unwrap();

        service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap();
        let mut settings = service.get();
        settings.provider_names.insert("codex".into(), "GPT".into());
        service
            .update_from_view(
                settings,
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();

        service
            .activate_account("codex", "codex", "bbbbbbbb22222222")
            .unwrap();
        assert!(!service.get().provider_names.contains_key("codex"));

        let mut settings = service.get();
        settings
            .provider_names
            .insert("codex".into(), "Work".into());
        service
            .update_from_view(
                settings,
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();

        service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap();
        assert_eq!(
            service
                .get()
                .provider_names
                .get("codex")
                .map(String::as_str),
            Some("GPT")
        );
        service
            .activate_account("codex", "codex", "bbbbbbbb22222222")
            .unwrap();
        assert_eq!(
            service
                .get()
                .provider_names
                .get("codex")
                .map(String::as_str),
            Some("Work")
        );

        let records = storage.load_provider_account_records("codex").unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .any(|(identity, provider_id, _)| identity == "aaaaaaaa11111111"
                && provider_id == "codex"));
        assert!(records
            .iter()
            .any(|(identity, provider_id, _)| identity == "bbbbbbbb22222222"
                && provider_id == "codex@bbbbbbbb"));
    }

    #[test]
    fn account_revision_only_changes_when_the_active_identity_changes() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service =
            SettingsService::new_for_test(storage, catalog(), &HashSet::from(["codex".to_owned()]))
                .unwrap();
        let initial_revision = service.account_revision();

        assert!(service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap());
        assert_eq!(service.account_revision(), initial_revision + 1);
        assert!(!service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap());
        assert_eq!(service.account_revision(), initial_revision + 1);
        assert!(service
            .activate_account("codex", "codex", "bbbbbbbb22222222")
            .unwrap());
        assert_eq!(service.account_revision(), initial_revision + 2);
    }

    #[test]
    fn settings_revision_rejects_a_stale_full_snapshot() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(storage, catalog(), &HashSet::new()).unwrap();
        let initial_revision = service.settings_revision();
        let stale = service.get();
        let mut newer = stale.clone();
        newer.density = crate::models::DensityPreference::Compact;

        service
            .update_from_view(newer, initial_revision, service.account_revision())
            .unwrap();
        let revision_after_save = service.settings_revision();
        assert_eq!(revision_after_save, initial_revision + 1);

        let error = service
            .update_from_view(stale, initial_revision, service.account_revision())
            .unwrap_err();

        assert!(error.contains("Settings changed"));
        assert_eq!(service.settings_revision(), revision_after_save);
        assert_eq!(
            service.get().density,
            crate::models::DensityPreference::Compact
        );
    }

    #[test]
    fn settings_revision_tracks_each_persisted_mutation_path() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(storage, catalog(), &HashSet::new()).unwrap();
        let mut revision = service.settings_revision();

        service.update(service.get()).unwrap();
        revision += 1;
        assert_eq!(service.settings_revision(), revision);

        service.record_provider_credential_mutation();
        service
            .reconcile_provider_credential_state("openrouter", true, true)
            .unwrap();
        revision += 1;
        assert_eq!(service.settings_revision(), revision);

        let plan = service.reset_detection_plan();
        service
            .apply_credential_detection(&plan, &probe_results(&[]))
            .unwrap();
        revision += 1;
        assert_eq!(service.settings_revision(), revision);

        assert!(service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap());
        revision += 1;
        assert_eq!(service.settings_revision(), revision);

        service
            .reset_provider(
                "codex",
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();
        revision += 1;
        assert_eq!(service.settings_revision(), revision);
        assert_eq!(
            service
                .view_state("prompt", None, true, None)
                .settings_revision,
            revision
        );
    }

    #[test]
    fn unchanged_credential_probe_does_not_create_a_settings_revision() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service =
            SettingsService::new_for_test(storage, catalog(), &HashSet::from(["codex".to_owned()]))
                .unwrap();
        let revision = service.settings_revision();
        let plan = service.reset_detection_plan();

        let outcome = service
            .apply_credential_detection(&plan, &probe_results(&["codex"]))
            .unwrap();

        assert!(outcome.newly_enabled_provider_ids.is_empty());
        assert_eq!(service.settings_revision(), revision);
    }

    #[test]
    fn stale_settings_save_cannot_move_a_name_to_the_new_active_account() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service =
            SettingsService::new_for_test(storage, catalog(), &HashSet::from(["codex".to_owned()]))
                .unwrap();
        service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap();
        let mut named = service.get();
        named.provider_names.insert("codex".into(), "GPT".into());
        service
            .update_from_view(
                named,
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();

        let stale_account_revision = service.account_revision();
        let mut stale_settings = service.get();
        service
            .activate_account("codex", "codex", "bbbbbbbb22222222")
            .unwrap();
        stale_settings.theme = ThemePreference::Dark;
        stale_settings
            .provider_names
            .insert("claude".into(), "Personal".into());
        let updated = service
            .update_from_view(
                stale_settings,
                service.settings_revision(),
                stale_account_revision,
            )
            .unwrap();

        assert_eq!(updated.theme, ThemePreference::Dark);
        assert!(!updated.provider_names.contains_key("codex"));
        assert_eq!(
            updated.provider_names.get("claude").map(String::as_str),
            Some("Personal")
        );
        service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap();
        assert_eq!(
            service
                .get()
                .provider_names
                .get("codex")
                .map(String::as_str),
            Some("GPT")
        );
    }

    #[test]
    fn codex_name_migration_keeps_the_original_accounts_name_after_an_offline_swap() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        storage
            .save_provider_account_record("codex", "aaaaaaaa11111111", "codex", "{}")
            .unwrap();
        let mut saved = default_settings(&catalog(), &HashSet::from(["codex".to_owned()]));
        saved.provider_names.insert("codex".into(), "GPT".into());
        storage.save_settings(&saved).unwrap();
        let service = SettingsService::new_for_test(
            storage.clone(),
            catalog(),
            &HashSet::from(["codex".to_owned()]),
        )
        .unwrap();

        service
            .activate_account("codex", "codex", "bbbbbbbb22222222")
            .unwrap();
        assert!(!service.get().provider_names.contains_key("codex"));
        service
            .activate_account("codex", "codex", "aaaaaaaa11111111")
            .unwrap();
        assert_eq!(
            service
                .get()
                .provider_names
                .get("codex")
                .map(String::as_str),
            Some("GPT")
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
            .apply_credential_detection(&plan, &probe_results(&["antigravity"]))
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
    fn normalization_preserves_valid_pins_for_dashboard_hidden_metrics() {
        let detected = HashSet::from(["codex".to_owned()]);
        let catalog = catalog();
        let mut settings = default_settings(&catalog, &detected);
        let codex = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "codex")
            .unwrap();
        let pinned = codex
            .metrics
            .iter_mut()
            .find(|metric| metric.pinned)
            .unwrap();
        pinned.enabled = false;
        let pinned_id = pinned.id.clone();

        normalize(&catalog, &mut settings, &detected);

        let pinned = settings
            .providers
            .iter()
            .find(|provider| provider.id == "codex")
            .unwrap()
            .metrics
            .iter()
            .find(|metric| metric.id == pinned_id)
            .unwrap();
        assert!(!pinned.enabled);
        assert!(pinned.pinned);
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
        settings.window_mode = crate::models::WindowMode::Floating;
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
    fn new_claude_account_is_inserted_after_its_provider_family() {
        let base = catalog();
        let mut settings = default_settings(&base, &HashSet::from(["claude".to_owned()]));

        normalize(
            &catalog_with_claude_account(),
            &mut settings,
            &HashSet::from(["claude".to_owned(), "claude@1234abcd".to_owned()]),
        );

        assert_eq!(
            settings
                .providers
                .iter()
                .take(3)
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "claude@1234abcd", "codex"]
        );
        assert!(settings
            .providers
            .iter()
            .find(|provider| provider.id == "claude@1234abcd")
            .is_some_and(|provider| provider.enabled && provider.detected));
    }

    #[test]
    fn unavailable_claude_account_keeps_its_customization_until_it_returns() {
        let account_catalog = catalog_with_claude_account();
        let detected = HashSet::from(["claude@1234abcd".to_owned()]);
        let mut settings = default_settings(&account_catalog, &detected);
        let account = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "claude@1234abcd")
            .unwrap();
        account.enabled = false;
        account.expanded = true;
        account.metrics.reverse();
        let expected_metrics = account.metrics.clone();

        let persisted = HashSet::from(["claude@1234abcd".to_owned()]);
        normalize_with_persisted_accounts(&catalog(), &mut settings, &HashSet::new(), &persisted);
        let unavailable = settings
            .providers
            .iter()
            .find(|provider| provider.id == "claude@1234abcd")
            .unwrap();
        assert!(!unavailable.detected);
        assert!(!unavailable.enabled);
        assert!(unavailable.expanded);
        assert_eq!(unavailable.metrics, expected_metrics);

        normalize(&account_catalog, &mut settings, &detected);
        let restored = settings
            .providers
            .iter()
            .find(|provider| provider.id == "claude@1234abcd")
            .unwrap();
        assert!(restored.detected);
        assert!(!restored.enabled);
        assert!(restored.expanded);
        assert_eq!(restored.metrics, expected_metrics);
    }

    #[test]
    fn account_shaped_layout_without_a_persisted_record_is_removed() {
        let account_catalog = catalog_with_claude_account();
        let mut settings = default_settings(
            &account_catalog,
            &HashSet::from(["claude@1234abcd".to_owned()]),
        );

        normalize_with_persisted_accounts(
            &catalog(),
            &mut settings,
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(!settings
            .providers
            .iter()
            .any(|provider| provider.id == "claude@1234abcd"));
    }

    #[test]
    fn swapped_default_keeps_the_absent_bare_accounts_customization() {
        let base = catalog();
        let mut settings = default_settings(&base, &HashSet::from(["claude".to_owned()]));
        let original = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == "claude")
            .unwrap();
        original.expanded = true;
        original.metrics.reverse();
        let expected_metrics = original.metrics.clone();
        settings
            .provider_names
            .insert("claude".into(), "Personal".into());

        let mut current_default = claude_account_definition();
        current_default.fallback_enabled = true;
        let swapped_catalog = Arc::new(
            ProviderRegistry::new(
                [
                    current_default,
                    codex::definition(),
                    cursor::definition(),
                    antigravity::definition(),
                    openrouter::definition(),
                ]
                .into_iter()
                .map(|definition| Arc::new(CatalogProvider(definition)) as Arc<dyn UsageProvider>)
                .collect(),
            )
            .unwrap(),
        );
        let persisted = HashSet::from(["claude".to_owned(), "claude@1234abcd".to_owned()]);
        normalize_with_persisted_accounts(
            &swapped_catalog,
            &mut settings,
            &HashSet::from(["claude@1234abcd".to_owned()]),
            &persisted,
        );

        let unavailable = settings
            .providers
            .iter()
            .find(|provider| provider.id == "claude")
            .unwrap();
        assert!(!unavailable.detected);
        assert!(unavailable.expanded);
        assert_eq!(unavailable.metrics, expected_metrics);
        assert_eq!(
            settings.provider_names.get("claude").map(String::as_str),
            Some("Personal")
        );
        assert!(settings
            .providers
            .iter()
            .find(|provider| provider.id == "claude@1234abcd")
            .is_some_and(|provider| provider.detected));
    }

    #[test]
    fn normalization_keeps_only_valid_account_card_names() {
        let registry = catalog_with_claude_account();
        let mut settings = default_settings(
            &registry,
            &HashSet::from(["claude".to_owned(), "claude@1234abcd".to_owned()]),
        );
        settings
            .provider_names
            .insert("claude".into(), "  Personal  ".into());
        settings
            .provider_names
            .insert("claude@1234abcd".into(), "  Work  ".into());
        settings
            .provider_names
            .insert("codex".into(), "  Work Codex  ".into());
        settings
            .provider_names
            .insert("claude@invalid".into(), "Invalid".into());
        settings
            .provider_names
            .insert("claude@deadbeef".into(), "Never observed".into());

        normalize(
            &registry,
            &mut settings,
            &HashSet::from(["claude".to_owned(), "claude@1234abcd".to_owned()]),
        );

        assert_eq!(settings.provider_names.len(), 3);
        assert_eq!(
            settings.provider_names.get("claude").map(String::as_str),
            Some("Personal")
        );
        assert_eq!(
            settings
                .provider_names
                .get("claude@1234abcd")
                .map(String::as_str),
            Some("Work")
        );
        assert_eq!(
            settings.provider_names.get("codex").map(String::as_str),
            Some("Work Codex")
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
        assert_eq!(settings.schema_version, 7);
        assert_eq!(
            settings
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex", "cursor", "antigravity", "openrouter"]
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

        let reset = service
            .reset_provider(
                "codex",
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();
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

    #[test]
    fn full_reset_restores_defaults_without_deleting_usage_or_accounts() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        storage
            .save_provider_account_record(
                "codex",
                "identity-a",
                "codex",
                r#"{"customName":"Work","plan":"plus"}"#,
            )
            .unwrap();
        storage
            .save_provider_account_record(
                "codex",
                "identity-b",
                "codex@bbbbbbbb",
                r#"{"customName":"Personal","region":"eu"}"#,
            )
            .unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: Some("Plus".into()),
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: Default::default(),
            warnings: Vec::new(),
            refreshed_at: chrono::Utc::now(),
        };
        storage.save_snapshot(&snapshot).unwrap();
        let service = SettingsService::new_for_test(
            storage.clone(),
            catalog(),
            &HashSet::from(["codex".to_owned()]),
        )
        .unwrap();
        service
            .activate_account("codex", "codex", "identity-a")
            .unwrap();
        let mut customized = service.get();
        customized.theme = ThemePreference::Dark;
        customized.density = crate::models::DensityPreference::Compact;
        customized.reduce_animations = true;
        customized.launch_at_login = true;
        customized.global_shortcut = Some("Ctrl+Shift+Q".into());
        customized.notifications.almost_out = true;
        customized
            .provider_names
            .insert("codex".into(), "Work".into());
        customized.providers.reverse();
        service.update(customized).unwrap();

        let defaults = service.reset_defaults();
        let reset = service
            .reset_all_from_view(
                defaults,
                service.settings_revision(),
                service.account_revision(),
            )
            .unwrap();

        let expected = service.reset_defaults();
        assert_eq!(reset.providers, expected.providers);
        assert_eq!(reset.provider_names, expected.provider_names);
        assert_eq!(reset.theme, expected.theme);
        assert_eq!(reset.density, expected.density);
        assert_eq!(reset.reduce_animations, expected.reduce_animations);
        assert_eq!(reset.launch_at_login, expected.launch_at_login);
        assert_eq!(reset.global_shortcut, expected.global_shortcut);
        assert_eq!(reset.notifications, expected.notifications);
        assert_eq!(storage.load_snapshot("codex").unwrap(), Some(snapshot));
        let records = storage.load_provider_account_records("codex").unwrap();
        assert_eq!(records.len(), 2);
        let payloads = records
            .into_iter()
            .map(|(identity, provider_id, payload)| {
                (
                    identity,
                    provider_id,
                    serde_json::from_str::<Value>(&payload).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(payloads[0].0, "identity-a");
        assert_eq!(payloads[0].1, "codex");
        assert_eq!(payloads[0].2["plan"], "plus");
        assert!(payloads[0].2.get("customName").is_none());
        assert_eq!(payloads[1].0, "identity-b");
        assert_eq!(payloads[1].1, "codex@bbbbbbbb");
        assert_eq!(payloads[1].2["region"], "eu");
        assert!(payloads[1].2.get("customName").is_none());
    }

    #[test]
    fn api_key_save_marks_and_enables_provider_while_delete_only_clears_detection() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(storage, catalog(), &HashSet::new()).unwrap();

        service.record_provider_credential_mutation();
        let saved = service
            .reconcile_provider_credential_state("openrouter", true, true)
            .unwrap();
        let openrouter = saved
            .providers
            .iter()
            .find(|provider| provider.id == "openrouter")
            .unwrap();
        assert!(openrouter.detected);
        assert!(openrouter.enabled);

        service.record_provider_credential_mutation();
        let deleted = service
            .reconcile_provider_credential_state("openrouter", false, false)
            .unwrap();
        let openrouter = deleted
            .providers
            .iter()
            .find(|provider| provider.id == "openrouter")
            .unwrap();
        assert!(!openrouter.detected);
        assert!(openrouter.enabled);
    }

    #[test]
    fn stale_absent_probe_cannot_undo_an_api_key_save_for_a_detected_provider() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(
            storage,
            catalog(),
            &HashSet::from(["openrouter".to_owned()]),
        )
        .unwrap();
        let plan = service.reset_detection_plan();

        service.record_provider_credential_mutation();
        service
            .reconcile_provider_credential_state("openrouter", true, true)
            .unwrap();
        let outcome = service
            .apply_credential_detection(&plan, &probe_results(&[]))
            .unwrap();
        let openrouter = outcome
            .settings
            .providers
            .iter()
            .find(|provider| provider.id == "openrouter")
            .unwrap();

        assert!(openrouter.detected);
        assert!(openrouter.enabled);
        assert_eq!(service.credential_revision.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stale_detected_probe_cannot_undo_an_api_key_delete() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let service = SettingsService::new_for_test(
            storage,
            catalog(),
            &HashSet::from(["openrouter".to_owned()]),
        )
        .unwrap();
        let plan = service.reset_detection_plan();

        service.record_provider_credential_mutation();
        service
            .reconcile_provider_credential_state("openrouter", false, false)
            .unwrap();
        let outcome = service
            .apply_credential_detection(&plan, &probe_results(&["openrouter"]))
            .unwrap();
        let openrouter = outcome
            .settings
            .providers
            .iter()
            .find(|provider| provider.id == "openrouter")
            .unwrap();

        assert!(!openrouter.detected);
        assert!(openrouter.enabled);
        assert_eq!(service.credential_revision.load(Ordering::SeqCst), 1);
    }
}
