use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use chrono::Utc;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    models::{ProviderSnapshot, ProviderViewState, SnapshotSource},
    providers::UsageProvider,
    storage::Storage,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const STALE_AFTER: chrono::Duration = chrono::Duration::minutes(10);

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageViewState {
    pub providers: BTreeMap<String, ProviderViewState>,
}

pub struct ProviderService {
    providers: BTreeMap<String, Arc<dyn UsageProvider>>,
    storage: Arc<Storage>,
    states: RwLock<BTreeMap<String, ProviderViewState>>,
    refresh_gates: HashMap<String, Arc<AsyncMutex<()>>>,
    last_live_refresh: Mutex<HashMap<String, Instant>>,
}

impl ProviderService {
    pub fn new(providers: Vec<Arc<dyn UsageProvider>>, storage: Arc<Storage>) -> Self {
        let mut provider_map = BTreeMap::new();
        let mut states = BTreeMap::new();
        let mut refresh_gates = HashMap::new();
        for provider in providers {
            let id = provider.id().to_owned();
            let state = storage
                .load_snapshot(&id)
                .ok()
                .flatten()
                .map(ProviderViewState::from_cache)
                .unwrap_or_default();
            states.insert(id.clone(), state);
            refresh_gates.insert(id.clone(), Arc::new(AsyncMutex::new(())));
            provider_map.insert(id, provider);
        }
        Self {
            providers: provider_map,
            storage,
            states: RwLock::new(states),
            refresh_gates,
            last_live_refresh: Mutex::new(HashMap::new()),
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
        UsageViewState { providers }
    }

    pub async fn refresh(&self, provider_id: &str, force: bool) -> ProviderViewState {
        let Some(provider) = self.providers.get(provider_id).cloned() else {
            return ProviderViewState {
                error: Some("Unknown provider.".into()),
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
            state.last_attempt_at = Some(Utc::now());
        });

        let result = tauri::async_runtime::spawn_blocking(move || provider.refresh()).await;
        let refresh_result = match result {
            Ok(result) => result,
            Err(_) => Err("Provider refresh stopped unexpectedly.".to_owned()),
        };
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
        result: Result<ProviderSnapshot, String>,
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

fn merge_refresh_result(state: &mut ProviderViewState, result: Result<ProviderSnapshot, String>) {
    match result {
        Ok(snapshot) => {
            state.snapshot = Some(snapshot);
            state.source = SnapshotSource::Live;
            state.error = None;
            state.stale = false;
        }
        Err(error) => {
            state.error = Some(error);
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

    use super::{merge_refresh_result, ProviderService};
    use crate::{
        models::{ProviderSnapshot, ProviderViewState, UsageHistory},
        providers::UsageProvider,
        storage::Storage,
    };

    struct SlowProvider {
        id: &'static str,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
    }

    impl UsageProvider for SlowProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn has_local_credentials(&self) -> bool {
            true
        }

        fn refresh(&self) -> Result<ProviderSnapshot, String> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(75));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ProviderSnapshot {
                provider_id: self.id.into(),
                plan: None,
                quotas: Vec::new(),
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
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };
        let mut state = ProviderViewState {
            snapshot: Some(snapshot.clone()),
            ..ProviderViewState::default()
        };
        merge_refresh_result(&mut state, Err("offline".into()));
        assert_eq!(state.snapshot, Some(snapshot));
        assert!(state.stale);
        assert_eq!(state.error.as_deref(), Some("offline"));
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
        let service = Arc::new(ProviderService::new(providers, storage));

        tauri::async_runtime::block_on(
            service.refresh_enabled(&["claude".into(), "antigravity".into()], true),
        );

        assert_eq!(maximum.load(Ordering::SeqCst), 2);
    }
}
