use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use chrono::Utc;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    models::{
        MetricSource, ProviderErrorKind, ProviderSnapshot, ProviderViewState, SnapshotSource,
    },
    policy::{REFRESH_INTERVAL, STALE_AFTER},
    providers::{ProviderError, ProviderRegistry},
    storage::Storage,
};

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageViewState {
    pub providers: BTreeMap<String, ProviderViewState>,
    pub last_full_refresh_at: Option<chrono::DateTime<Utc>>,
}

pub struct ProviderService {
    registry: Arc<ProviderRegistry>,
    storage: Arc<Storage>,
    states: RwLock<BTreeMap<String, ProviderViewState>>,
    refresh_gates: HashMap<String, Arc<AsyncMutex<()>>>,
    last_live_refresh: Mutex<HashMap<String, Instant>>,
    last_full_refresh_at: RwLock<Option<chrono::DateTime<Utc>>>,
}

impl ProviderService {
    pub fn new(registry: Arc<ProviderRegistry>, storage: Arc<Storage>) -> Self {
        let mut states = BTreeMap::new();
        let mut refresh_gates = HashMap::new();
        for definition in &registry.catalog().providers {
            let id = definition.id.clone();
            let state = storage
                .load_snapshot(&id)
                .ok()
                .flatten()
                .map(ProviderViewState::from_cache)
                .unwrap_or_default();
            states.insert(id.clone(), state);
            refresh_gates.insert(id.clone(), Arc::new(AsyncMutex::new(())));
        }
        Self {
            registry,
            storage,
            states: RwLock::new(states),
            refresh_gates,
            last_live_refresh: Mutex::new(HashMap::new()),
            last_full_refresh_at: RwLock::new(None),
        }
    }

    pub fn state(&self) -> UsageViewState {
        let mut providers = self
            .states
            .read()
            .map(|value| value.clone())
            .unwrap_or_default();
        for state in providers.values_mut() {
            if let Some(snapshot) = state.snapshot.as_ref() {
                state.stale |=
                    Utc::now().signed_duration_since(snapshot.refreshed_at) >= STALE_AFTER;
            }
        }
        let last_full_refresh_at = self
            .last_full_refresh_at
            .read()
            .ok()
            .and_then(|value| value.to_owned());
        UsageViewState {
            providers,
            last_full_refresh_at,
        }
    }

    pub async fn refresh(&self, provider_id: &str, force: bool) -> ProviderViewState {
        let Some(provider) = self.registry.runtime(provider_id) else {
            return ProviderViewState {
                error: Some("Unknown provider.".into()),
                error_kind: Some(ProviderErrorKind::Internal),
                ..ProviderViewState::default()
            };
        };
        let Some(gate) = self.refresh_gates.get(provider_id).cloned() else {
            return ProviderViewState::default();
        };
        let _guard = gate.lock().await;
        if !force && self.is_fresh_this_session(provider_id) {
            return self.provider_state(provider_id);
        }
        self.update_state(provider_id, |state| {
            state.refreshing = true;
            state.error = None;
            state.error_kind = None;
            state.last_attempt_at = Some(Utc::now());
        });

        let result = tauri::async_runtime::spawn_blocking(move || provider.refresh()).await;
        let refresh_result = match result {
            Ok(result) => result,
            Err(_) => Err(ProviderError::new(
                ProviderErrorKind::Internal,
                "Provider refresh stopped unexpectedly.",
            )),
        };
        let refresh_result = refresh_result
            .and_then(|snapshot| validate_snapshot(&self.registry, provider_id, snapshot));
        let state = self.apply_refresh_result(provider_id, refresh_result);
        if state.error.is_none() {
            if let Ok(mut last) = self.last_live_refresh.lock() {
                last.insert(provider_id.to_owned(), Instant::now());
            }
        }
        state
    }

    pub async fn refresh_enabled(
        self: &Arc<Self>,
        provider_ids: &[String],
        force: bool,
    ) -> UsageViewState {
        let mut tasks = Vec::with_capacity(provider_ids.len());
        for provider_id in provider_ids {
            let service = self.clone();
            let provider_id = provider_id.clone();
            tasks.push(tauri::async_runtime::spawn(async move {
                service.refresh(&provider_id, force).await
            }));
        }
        for task in tasks {
            let _ = task.await;
        }
        self.state()
    }

    pub async fn refresh_all(
        self: &Arc<Self>,
        provider_ids: &[String],
        force: bool,
    ) -> UsageViewState {
        self.refresh_enabled(provider_ids, force).await;
        if let Ok(mut completed_at) = self.last_full_refresh_at.write() {
            *completed_at = Some(Utc::now());
        }
        self.state()
    }

    fn provider_state(&self, provider_id: &str) -> ProviderViewState {
        self.states
            .read()
            .ok()
            .and_then(|states| states.get(provider_id).cloned())
            .unwrap_or_default()
    }

    fn apply_refresh_result(
        &self,
        provider_id: &str,
        result: Result<ProviderSnapshot, ProviderError>,
    ) -> ProviderViewState {
        let cache_error = result
            .as_ref()
            .ok()
            .is_some_and(|snapshot| self.storage.save_snapshot(snapshot).is_err());
        self.update_state(provider_id, |state| {
            merge_refresh_result(state, result);
            if cache_error {
                state.error = Some(
                    "Usage refreshed, but the last successful snapshot could not be cached.".into(),
                );
                state.error_kind = Some(ProviderErrorKind::Storage);
            }
        });
        self.provider_state(provider_id)
    }

    fn is_fresh_this_session(&self, provider_id: &str) -> bool {
        self.last_live_refresh
            .lock()
            .ok()
            .and_then(|value| value.get(provider_id).copied())
            .is_some_and(|instant| instant.elapsed() < REFRESH_INTERVAL)
    }

    fn update_state(&self, provider_id: &str, update: impl FnOnce(&mut ProviderViewState)) {
        if let Ok(mut states) = self.states.write() {
            update(states.entry(provider_id.to_owned()).or_default());
        }
    }
}

fn validate_snapshot(
    registry: &ProviderRegistry,
    provider_id: &str,
    snapshot: ProviderSnapshot,
) -> Result<ProviderSnapshot, ProviderError> {
    let Some(definition) = registry.definition(provider_id) else {
        return Err(snapshot_contract_error());
    };
    if snapshot.provider_id != provider_id {
        return Err(snapshot_contract_error());
    }

    let quota_sources = definition
        .metrics
        .iter()
        .filter_map(|metric| match &metric.source {
            MetricSource::Quota { source_id, .. }
            | MetricSource::QuotaOrValue { source_id, .. } => Some(source_id.as_str()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let value_sources = definition
        .metrics
        .iter()
        .filter_map(|metric| match &metric.source {
            MetricSource::Value { source_id } | MetricSource::QuotaOrValue { source_id, .. } => {
                Some(source_id.as_str())
            }
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    if snapshot
        .quotas
        .iter()
        .any(|quota| !quota_sources.contains(quota.id.as_str()))
        || snapshot
            .value_metrics
            .iter()
            .any(|metric| !value_sources.contains(metric.id.as_str()))
    {
        return Err(snapshot_contract_error());
    }
    Ok(snapshot)
}

fn snapshot_contract_error() -> ProviderError {
    ProviderError::new(
        ProviderErrorKind::Internal,
        "Provider data does not match its registered metric contract.",
    )
}

fn merge_refresh_result(
    state: &mut ProviderViewState,
    result: Result<ProviderSnapshot, ProviderError>,
) {
    match result {
        Ok(snapshot) => {
            state.snapshot = Some(snapshot);
            state.source = SnapshotSource::Live;
            state.error = None;
            state.error_kind = None;
            state.stale = false;
        }
        Err(error) => {
            state.error_kind = Some(error.kind());
            state.error = Some(error.to_string());
            state.stale = state.snapshot.is_some();
        }
    }
    state.refreshing = false;
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    use chrono::Utc;
    use tempfile::tempdir;

    use super::{merge_refresh_result, validate_snapshot, ProviderService};
    use crate::{
        models::{
            MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderErrorKind,
            ProviderSnapshot, ProviderViewState, UsageHistory,
        },
        providers::{ProviderError, ProviderRegistry, UsageProvider},
        storage::Storage,
    };

    struct SlowProvider {
        id: &'static str,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
    }

    impl UsageProvider for SlowProvider {
        fn definition(&self) -> ProviderDefinition {
            ProviderDefinition {
                id: self.id.into(),
                display_name: self.id.into(),
                short_name: "T".into(),
                fallback_enabled: true,
                local_usage_source_note: None,
                metrics: vec![MetricDefinition::new(
                    format!("{}.session", self.id),
                    "Session",
                    MetricSource::Quota {
                        source_id: "session".into(),
                        session_window: false,
                    },
                    true,
                    true,
                    MetricSection::AlwaysVisible,
                    true,
                    Some("S"),
                    None,
                )],
            }
        }

        fn has_local_credentials(&self) -> bool {
            true
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(75));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ProviderSnapshot {
                provider_id: self.id.into(),
                plan: None,
                quotas: Vec::new(),
                value_metrics: Vec::new(),
                notices: Vec::new(),
                usage: UsageHistory::default(),
                warnings: Vec::new(),
                refreshed_at: Utc::now(),
            })
        }
    }

    #[test]
    fn failed_refresh_preserves_last_successful_snapshot() {
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: None,
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };
        let mut state = ProviderViewState {
            snapshot: Some(snapshot.clone()),
            ..ProviderViewState::default()
        };
        merge_refresh_result(
            &mut state,
            Err(ProviderError::new(ProviderErrorKind::Network, "offline")),
        );
        assert_eq!(state.snapshot, Some(snapshot));
        assert!(state.stale);
        assert_eq!(state.error.as_deref(), Some("offline"));
        assert_eq!(state.error_kind, Some(ProviderErrorKind::Network));
    }

    #[test]
    fn snapshot_contract_rejects_wrong_provider_and_unknown_sources() {
        let provider = SlowProvider {
            id: "contract",
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
        };
        let registry = ProviderRegistry::from_definitions(vec![provider.definition()]).unwrap();
        let snapshot = provider.refresh().unwrap();

        let mut wrong_provider = snapshot.clone();
        wrong_provider.provider_id = "other".into();
        assert!(validate_snapshot(&registry, "contract", wrong_provider).is_err());

        let mut unknown_source = snapshot;
        unknown_source.quotas.push(crate::models::QuotaWindow {
            id: "unknown".into(),
            label: "Unknown".into(),
            used_percent: 0.0,
            resets_at: None,
            period_seconds: 1,
            format: crate::models::QuotaFormat::Percent,
            used_value: None,
            limit_value: None,
        });
        assert!(validate_snapshot(&registry, "contract", unknown_source).is_err());
    }

    #[test]
    fn enabled_providers_refresh_in_parallel() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let providers = ["claude", "antigravity"]
            .into_iter()
            .map(|id| {
                Arc::new(SlowProvider {
                    id,
                    active: active.clone(),
                    maximum: maximum.clone(),
                }) as Arc<dyn UsageProvider>
            })
            .collect();
        let registry = Arc::new(ProviderRegistry::new(providers).unwrap());
        let service = Arc::new(ProviderService::new(registry, storage));

        tauri::async_runtime::block_on(
            service.refresh_enabled(&["claude".into(), "antigravity".into()], true),
        );

        assert_eq!(maximum.load(Ordering::SeqCst), 2);
        assert!(service.state().last_full_refresh_at.is_none());

        let completed = tauri::async_runtime::block_on(
            service.refresh_all(&["claude".into(), "antigravity".into()], true),
        );
        assert!(completed.last_full_refresh_at.is_some());
    }
}
