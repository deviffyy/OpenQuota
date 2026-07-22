use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use chrono::Utc;

use crate::{
    models::{
        MetricSource, ProviderErrorKind, ProviderSnapshot, ProviderViewState, SnapshotSource,
    },
    policy::{FAILURE_RETRY_BACKOFF, REFRESH_INTERVAL, STALE_AFTER},
    providers::{ProviderError, ProviderRegistry},
    storage::Storage,
};

const PROVIDER_REFRESH_TIMEOUT: Duration = Duration::from_secs(45);

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
    refresh_flights: HashMap<String, Arc<RefreshFlight>>,
    last_live_refresh: Mutex<HashMap<String, Instant>>,
    last_failed_refresh: Mutex<HashMap<String, Instant>>,
    last_full_refresh_at: RwLock<Option<chrono::DateTime<Utc>>>,
    refresh_timeout: Duration,
}

impl ProviderService {
    pub fn new(registry: Arc<ProviderRegistry>, storage: Arc<Storage>) -> Self {
        Self::with_refresh_timeout(registry, storage, PROVIDER_REFRESH_TIMEOUT)
    }

    fn with_refresh_timeout(
        registry: Arc<ProviderRegistry>,
        storage: Arc<Storage>,
        refresh_timeout: Duration,
    ) -> Self {
        let mut states = BTreeMap::new();
        let mut refresh_flights = HashMap::new();
        for definition in &registry.catalog().providers {
            let id = definition.id.clone();
            let state = match storage.load_snapshot(&id) {
                Ok(Some(snapshot)) => {
                    crate::app_debug!("cache", "loaded cached snapshot for {id}");
                    ProviderViewState::from_cache(snapshot)
                }
                Ok(None) => ProviderViewState::default(),
                Err(error) => {
                    crate::app_warn!(
                        "cache",
                        "cached snapshot for {id} could not be loaded: {error}"
                    );
                    ProviderViewState::default()
                }
            };
            states.insert(id.clone(), state);
            refresh_flights.insert(id, Arc::new(RefreshFlight::new()));
        }
        Self {
            registry,
            storage,
            states: RwLock::new(states),
            refresh_flights,
            last_live_refresh: Mutex::new(HashMap::new()),
            last_failed_refresh: Mutex::new(HashMap::new()),
            last_full_refresh_at: RwLock::new(None),
            refresh_timeout,
        }
    }

    pub fn state(&self) -> UsageViewState {
        let mut providers = self
            .states
            .read()
            .map(|value| value.clone())
            .unwrap_or_default();
        for state in providers.values_mut() {
            update_staleness_from_snapshot_age(state);
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

    pub async fn refresh(self: &Arc<Self>, provider_id: &str, force: bool) -> ProviderViewState {
        if self.registry.runtime(provider_id).is_none() {
            crate::app_error!(
                "refresh",
                "refresh requested for unknown provider {provider_id}"
            );
            return ProviderViewState {
                error: Some("Unknown provider.".into()),
                error_kind: Some(ProviderErrorKind::Internal),
                ..ProviderViewState::default()
            };
        };
        if !force && self.is_fresh_this_session(provider_id) {
            crate::app_debug!("refresh", "cache hit {provider_id}");
            return self.provider_state(provider_id);
        }
        if !force && self.is_in_failure_backoff(provider_id) {
            crate::app_debug!(
                "refresh",
                "backoff skip {provider_id} (failed <{}s ago)",
                FAILURE_RETRY_BACKOFF.as_secs()
            );
            return self.provider_state(provider_id);
        }
        let Some(flight) = self.refresh_flights.get(provider_id).cloned() else {
            return self.provider_state(provider_id);
        };
        let mut completed = flight.completed_tx.subscribe();
        let (target_generation, start_runner) = {
            let Ok(mut flight_state) = flight.state.lock() else {
                crate::app_error!(
                    "refresh",
                    "refresh coordination unavailable for {provider_id}"
                );
                return ProviderViewState {
                    error: Some("Provider refresh is temporarily unavailable.".into()),
                    error_kind: Some(ProviderErrorKind::Internal),
                    ..self.provider_state(provider_id)
                };
            };
            if !flight_state.runner_active {
                let generation = flight_state.completed_generation.saturating_add(1);
                flight_state.runner_active = true;
                flight_state.attempt_generation = Some(generation);
                flight_state.requested_generation = generation;
                (generation, true)
            } else if force {
                let generation = flight_state.attempt_generation.map_or_else(
                    || flight_state.completed_generation.saturating_add(1),
                    |active| active.saturating_add(1),
                );
                flight_state.requested_generation =
                    flight_state.requested_generation.max(generation);
                crate::app_debug!("refresh", "queued forced follow-up for {provider_id}");
                (flight_state.requested_generation, false)
            } else if let Some(generation) = flight_state.attempt_generation {
                (generation, false)
            } else {
                return self.provider_state(provider_id);
            }
        };

        if start_runner {
            let service = self.clone();
            let provider_id = provider_id.to_owned();
            let runner_flight = flight.clone();
            tauri::async_runtime::spawn(async move {
                service
                    .run_refresh_flight(provider_id, runner_flight, force)
                    .await;
            });
        }

        while *completed.borrow_and_update() < target_generation {
            if completed.changed().await.is_err() {
                break;
            }
        }
        self.provider_state(provider_id)
    }

    async fn run_refresh_flight(
        self: Arc<Self>,
        provider_id: String,
        flight: Arc<RefreshFlight>,
        initial_force: bool,
    ) {
        let Some(provider) = self.registry.runtime(&provider_id) else {
            return;
        };
        let mut force = initial_force;
        loop {
            let generation = flight
                .state
                .lock()
                .ok()
                .and_then(|state| state.attempt_generation)
                .unwrap_or_default();
            let started = Instant::now();
            let tag = format!("plugin:{provider_id}");
            crate::app_info!(&tag, "refresh start (force={force})");
            self.update_state(&provider_id, |state| {
                state.refreshing = true;
                state.error = None;
                state.error_kind = None;
                state.last_attempt_at = Some(Utc::now());
            });
            let worker_provider = provider.clone();
            let mut worker =
                tauri::async_runtime::spawn_blocking(move || worker_provider.refresh());
            let mut late_worker = None;
            let refresh_result = match tokio::time::timeout(self.refresh_timeout, &mut worker).await
            {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => {
                    crate::app_error!(&tag, "refresh worker stopped unexpectedly");
                    Err(ProviderError::new(
                        ProviderErrorKind::Internal,
                        "Provider refresh stopped unexpectedly.",
                    ))
                }
                Err(_) => {
                    crate::app_warn!(
                        &tag,
                        "refresh timed out after {}ms; late result will be discarded",
                        self.refresh_timeout.as_millis()
                    );
                    late_worker = Some(worker);
                    Err(ProviderError::new(
                        ProviderErrorKind::Network,
                        "Provider refresh timed out.",
                    ))
                }
            };
            let refresh_result = refresh_result
                .and_then(|snapshot| validate_snapshot(&self.registry, &provider_id, snapshot));
            match &refresh_result {
                Ok(_) => {
                    crate::app_info!(&tag, "refresh end ({}ms)", started.elapsed().as_millis())
                }
                Err(error) => crate::app_warn!(
                    &tag,
                    "refresh failed ({}ms, kind={:?}): {error}",
                    started.elapsed().as_millis(),
                    error.kind()
                ),
            }
            let state = self.apply_refresh_result(&provider_id, refresh_result);
            if state.error.is_none() {
                if let Ok(mut last) = self.last_live_refresh.lock() {
                    last.insert(provider_id.clone(), Instant::now());
                }
                if let Ok(mut failures) = self.last_failed_refresh.lock() {
                    failures.remove(&provider_id);
                }
            } else if let Ok(mut failures) = self.last_failed_refresh.lock() {
                failures.insert(provider_id.clone(), Instant::now());
            }

            if let Ok(mut flight_state) = flight.state.lock() {
                flight_state.completed_generation = generation;
                flight_state.attempt_generation = None;
            }
            flight.completed_tx.send_replace(generation);

            if let Some(worker) = late_worker {
                match worker.await {
                    Ok(_) => crate::app_debug!(
                        &tag,
                        "timed-out refresh worker finished; discarded late result"
                    ),
                    Err(_) => crate::app_debug!(
                        &tag,
                        "timed-out refresh worker stopped; discarded late failure"
                    ),
                }
            }

            let run_follow_up = if let Ok(mut flight_state) = flight.state.lock() {
                if flight_state.requested_generation > flight_state.completed_generation {
                    let generation = flight_state.completed_generation.saturating_add(1);
                    flight_state.attempt_generation = Some(generation);
                    true
                } else {
                    flight_state.runner_active = false;
                    false
                }
            } else {
                false
            };
            if !run_follow_up {
                return;
            }
            force = true;
        }
    }

    #[cfg(test)]
    async fn refresh_enabled(
        self: &Arc<Self>,
        provider_ids: &[String],
        force: bool,
    ) -> UsageViewState {
        self.refresh_enabled_with_progress(provider_ids, force, |_| {})
            .await
    }

    pub async fn refresh_enabled_with_progress<F>(
        self: &Arc<Self>,
        provider_ids: &[String],
        force: bool,
        mut on_progress: F,
    ) -> UsageViewState
    where
        F: FnMut(&UsageViewState) + Send,
    {
        let started = Instant::now();
        crate::app_info!(
            "refresh",
            "batch start ({} providers, force={force})",
            provider_ids.len()
        );
        let (completed_tx, mut completed_rx) = tokio::sync::mpsc::unbounded_channel();
        for provider_id in provider_ids {
            let service = self.clone();
            let provider_id = provider_id.clone();
            let completed_tx = completed_tx.clone();
            tauri::async_runtime::spawn(async move {
                let state = service.refresh(&provider_id, force).await;
                let _ = completed_tx.send(state);
            });
        }
        drop(completed_tx);

        let mut succeeded = 0;
        let mut failed = 0;
        let mut completed = 0;
        while let Some(state) = completed_rx.recv().await {
            completed += 1;
            if state.error.is_none() {
                succeeded += 1;
            } else {
                failed += 1;
            }
            let current = self.state();
            on_progress(&current);
        }
        failed += provider_ids.len().saturating_sub(completed);
        crate::app_info!(
            "refresh",
            "batch end ({}ms, {succeeded} ok / {failed} failed)",
            started.elapsed().as_millis()
        );
        self.state()
    }

    #[cfg(test)]
    async fn refresh_all(self: &Arc<Self>, provider_ids: &[String], force: bool) -> UsageViewState {
        self.refresh_all_with_progress(provider_ids, force, |_| {})
            .await
    }

    pub async fn refresh_all_with_progress<F>(
        self: &Arc<Self>,
        provider_ids: &[String],
        force: bool,
        on_progress: F,
    ) -> UsageViewState
    where
        F: FnMut(&UsageViewState) + Send,
    {
        self.refresh_enabled_with_progress(provider_ids, force, on_progress)
            .await;
        if let Ok(mut completed_at) = self.last_full_refresh_at.write() {
            *completed_at = Some(Utc::now());
        }
        self.state()
    }

    fn provider_state(&self, provider_id: &str) -> ProviderViewState {
        let mut state = self
            .states
            .read()
            .ok()
            .and_then(|states| states.get(provider_id).cloned())
            .unwrap_or_default();
        update_staleness_from_snapshot_age(&mut state);
        state
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
        if cache_error {
            crate::app_warn!("cache", "snapshot for {provider_id} could not be persisted");
        } else if result.is_ok() {
            crate::app_debug!("cache", "snapshot for {provider_id} persisted");
        }
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

    fn is_in_failure_backoff(&self, provider_id: &str) -> bool {
        self.last_failed_refresh
            .lock()
            .ok()
            .and_then(|value| value.get(provider_id).copied())
            .is_some_and(|instant| instant.elapsed() < FAILURE_RETRY_BACKOFF)
    }

    fn update_state(&self, provider_id: &str, update: impl FnOnce(&mut ProviderViewState)) {
        if let Ok(mut states) = self.states.write() {
            update(states.entry(provider_id.to_owned()).or_default());
        }
    }
}

struct RefreshFlight {
    state: Mutex<RefreshFlightState>,
    completed_tx: tokio::sync::watch::Sender<u64>,
}

#[derive(Default)]
struct RefreshFlightState {
    runner_active: bool,
    attempt_generation: Option<u64>,
    requested_generation: u64,
    completed_generation: u64,
}

impl RefreshFlight {
    fn new() -> Self {
        let (completed_tx, _) = tokio::sync::watch::channel(0);
        Self {
            state: Mutex::new(RefreshFlightState::default()),
            completed_tx,
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
    let status_sources = definition
        .metrics
        .iter()
        .filter_map(|metric| match &metric.source {
            MetricSource::Status { source_id } => Some(source_id.as_str()),
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
        || snapshot
            .status_metrics
            .iter()
            .any(|metric| !status_sources.contains(metric.id.as_str()))
        || has_duplicate_ids(snapshot.quotas.iter().map(|metric| metric.id.as_str()))
        || has_duplicate_ids(
            snapshot
                .value_metrics
                .iter()
                .map(|metric| metric.id.as_str()),
        )
        || has_duplicate_ids(
            snapshot
                .status_metrics
                .iter()
                .map(|metric| metric.id.as_str()),
        )
        || snapshot.quotas.iter().any(|quota| {
            (quota.format == crate::models::QuotaFormat::Count
                && quota
                    .unit
                    .as_deref()
                    .is_none_or(|unit| unit.trim().is_empty()))
                || (quota.estimated
                    && quota
                        .source_note
                        .as_deref()
                        .is_none_or(|note| note.trim().is_empty()))
        })
        || snapshot
            .status_metrics
            .iter()
            .any(|metric| metric.text.trim().is_empty() || metric.label.trim().is_empty())
    {
        return Err(snapshot_contract_error());
    }
    Ok(snapshot)
}

fn has_duplicate_ids<'a>(mut ids: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = std::collections::HashSet::new();
    ids.any(|id| !seen.insert(id))
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
        }
    }
    state.refreshing = false;
}

fn update_staleness_from_snapshot_age(state: &mut ProviderViewState) {
    state.stale = state.snapshot.as_ref().is_some_and(|snapshot| {
        Utc::now().signed_duration_since(snapshot.refreshed_at) >= STALE_AFTER
    });
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    use chrono::Utc;
    use tempfile::tempdir;

    use super::{merge_refresh_result, validate_snapshot, ProviderService};
    use crate::{
        models::{
            MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderErrorKind,
            ProviderSnapshot, ProviderViewState, QuotaFormat, QuotaWindow, SnapshotSource,
            StatusMetric, StatusTone, UsageHistory,
        },
        policy::{FAILURE_RETRY_BACKOFF, STALE_AFTER},
        providers::{ProviderError, ProviderRegistry, UsageProvider},
        storage::Storage,
    };

    struct SlowProvider {
        id: &'static str,
        calls: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        delay: Duration,
    }

    struct SequenceProvider {
        id: &'static str,
        calls: Arc<AtomicUsize>,
        failures_before_success: usize,
    }

    struct CredentialProvider {
        id: &'static str,
        credential: Arc<Mutex<Option<String>>>,
        calls: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl UsageProvider for SlowProvider {
        fn definition(&self) -> ProviderDefinition {
            test_definition(self.id)
        }

        fn has_local_credentials(&self) -> bool {
            true
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            thread::sleep(self.delay);
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(test_snapshot(self.id))
        }
    }

    impl UsageProvider for SequenceProvider {
        fn definition(&self) -> ProviderDefinition {
            test_definition(self.id)
        }

        fn has_local_credentials(&self) -> bool {
            true
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call <= self.failures_before_success {
                Err(ProviderError::new(ProviderErrorKind::Network, "offline"))
            } else {
                Ok(test_snapshot(self.id))
            }
        }
    }

    impl UsageProvider for CredentialProvider {
        fn definition(&self) -> ProviderDefinition {
            test_definition(self.id)
        }

        fn has_local_credentials(&self) -> bool {
            self.credential.lock().unwrap().is_some()
        }

        fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let credential = self.credential.lock().unwrap().clone();
            thread::sleep(self.delay);
            self.active.fetch_sub(1, Ordering::SeqCst);
            let mut snapshot = test_snapshot(self.id);
            snapshot.plan = credential;
            Ok(snapshot)
        }
    }

    fn test_definition(id: &str) -> ProviderDefinition {
        ProviderDefinition {
            id: id.into(),
            display_name: id.into(),
            short_name: "T".into(),
            fallback_enabled: true,
            local_usage_source_note: None,
            links: vec![],
            metrics: vec![MetricDefinition::new(
                format!("{id}.session"),
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

    fn test_snapshot(provider_id: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            provider_id: provider_id.into(),
            plan: None,
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        }
    }

    fn refresh_with_test_timeout(
        service: &Arc<ProviderService>,
        provider_id: &str,
        force: bool,
    ) -> ProviderViewState {
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(2), service.refresh(provider_id, force))
                .await
                .expect("refresh should not deadlock")
        })
    }

    #[test]
    fn failed_refresh_preserves_last_successful_snapshot_without_forcing_stale() {
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: None,
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
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
        assert!(!state.stale);
        assert_eq!(state.error.as_deref(), Some("offline"));
        assert_eq!(state.error_kind, Some(ProviderErrorKind::Network));
    }

    #[test]
    fn cached_snapshot_staleness_is_based_on_snapshot_age() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let provider = Arc::new(SlowProvider {
            id: "cached",
            calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
            delay: Duration::ZERO,
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());

        storage.save_snapshot(&test_snapshot("cached")).unwrap();
        let fresh_service = ProviderService::new(registry.clone(), storage.clone());
        let fresh = fresh_service.state();
        let fresh = fresh.providers.get("cached").unwrap();
        assert_eq!(fresh.source, SnapshotSource::Cache);
        assert!(!fresh.stale);

        let mut old_snapshot = test_snapshot("cached");
        old_snapshot.refreshed_at = Utc::now() - STALE_AFTER - chrono::Duration::seconds(1);
        storage.save_snapshot(&old_snapshot).unwrap();
        let old_service = ProviderService::new(registry, storage);
        let old = old_service.state();
        assert!(old.providers.get("cached").unwrap().stale);
    }

    #[test]
    fn snapshot_contract_rejects_wrong_provider_and_unknown_sources() {
        let provider = SlowProvider {
            id: "contract",
            calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
            delay: Duration::ZERO,
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
            unit: None,
            estimated: false,
            source_note: None,
        });
        assert!(validate_snapshot(&registry, "contract", unknown_source).is_err());
    }

    #[test]
    fn snapshot_contract_validates_dynamic_metric_metadata_and_unique_ids() {
        let definition = ProviderDefinition {
            id: "dynamic".into(),
            display_name: "Dynamic".into(),
            short_name: "D".into(),
            fallback_enabled: true,
            local_usage_source_note: None,
            links: Vec::new(),
            metrics: vec![
                MetricDefinition::quota(
                    "dynamic.searches",
                    "Web Searches",
                    "searches",
                    false,
                    true,
                    MetricSection::AlwaysVisible,
                    true,
                    "S",
                ),
                MetricDefinition::status(
                    "dynamic.extra",
                    "Extra Usage",
                    "extra",
                    true,
                    MetricSection::OnDemand,
                    false,
                    "E",
                ),
            ],
        };
        let registry = ProviderRegistry::from_definitions(vec![definition]).unwrap();
        let mut snapshot = test_snapshot("dynamic");
        snapshot.quotas.push(QuotaWindow {
            id: "searches".into(),
            label: "Web Searches".into(),
            used_percent: 25.0,
            resets_at: None,
            period_seconds: 86_400,
            format: QuotaFormat::Count,
            used_value: Some(25.0),
            limit_value: Some(100.0),
            unit: Some("searches".into()),
            estimated: false,
            source_note: None,
        });
        snapshot.status_metrics.push(StatusMetric {
            id: "extra".into(),
            label: "Extra Usage".into(),
            text: "2500 cap".into(),
            tone: StatusTone::Positive,
            subtitle: None,
        });

        assert!(validate_snapshot(&registry, "dynamic", snapshot.clone()).is_ok());

        let mut missing_unit = snapshot.clone();
        missing_unit.quotas[0].unit = Some(" ".into());
        assert!(validate_snapshot(&registry, "dynamic", missing_unit).is_err());

        let mut missing_estimate_source = snapshot.clone();
        missing_estimate_source.quotas[0].estimated = true;
        assert!(validate_snapshot(&registry, "dynamic", missing_estimate_source).is_err());

        let mut duplicate_quota = snapshot.clone();
        duplicate_quota
            .quotas
            .push(duplicate_quota.quotas[0].clone());
        assert!(validate_snapshot(&registry, "dynamic", duplicate_quota).is_err());

        let mut unknown_status = snapshot;
        unknown_status.status_metrics[0].id = "unknown".into();
        assert!(validate_snapshot(&registry, "dynamic", unknown_status).is_err());
    }

    #[test]
    fn enabled_providers_refresh_in_parallel() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = ["claude", "antigravity"]
            .into_iter()
            .map(|id| {
                Arc::new(SlowProvider {
                    id,
                    calls: calls.clone(),
                    active: active.clone(),
                    maximum: maximum.clone(),
                    delay: Duration::from_millis(75),
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

    #[test]
    fn timed_out_refresh_preserves_last_good_and_bounds_late_worker() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let mut cached = test_snapshot("slow");
        cached.plan = Some("cached".into());
        storage.save_snapshot(&cached).unwrap();

        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SlowProvider {
            id: "slow",
            calls: calls.clone(),
            active: active.clone(),
            maximum: maximum.clone(),
            delay: Duration::from_millis(140),
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::with_refresh_timeout(
            registry,
            storage.clone(),
            Duration::from_millis(25),
        ));

        let timed_out = refresh_with_test_timeout(&service, "slow", true);
        assert_eq!(
            timed_out.error.as_deref(),
            Some("Provider refresh timed out.")
        );
        assert_eq!(timed_out.error_kind, Some(ProviderErrorKind::Network));
        assert!(!timed_out.stale);
        assert_eq!(
            timed_out
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.plan.as_deref()),
            Some("cached")
        );

        let follow_up = refresh_with_test_timeout(&service, "slow", true);
        assert_eq!(follow_up.error, timed_out.error);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);

        thread::sleep(Duration::from_millis(180));
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(
            !service
                .refresh_flights
                .get("slow")
                .unwrap()
                .state
                .lock()
                .unwrap()
                .runner_active
        );
        assert_eq!(
            service
                .state()
                .providers
                .get("slow")
                .and_then(|state| state.snapshot.as_ref())
                .and_then(|snapshot| snapshot.plan.as_deref()),
            Some("cached")
        );
        assert_eq!(
            storage
                .load_snapshot("slow")
                .unwrap()
                .and_then(|snapshot| snapshot.plan),
            Some("cached".into())
        );
    }

    #[test]
    fn cancelled_refresh_keeps_single_flight_until_blocking_worker_finishes() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SlowProvider {
            id: "cancelled",
            calls: calls.clone(),
            active: active.clone(),
            maximum: maximum.clone(),
            delay: Duration::from_millis(180),
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::with_refresh_timeout(
            registry,
            storage,
            Duration::from_secs(1),
        ));

        let first_service = service.clone();
        let first =
            tauri::async_runtime::spawn(
                async move { first_service.refresh("cancelled", true).await },
            );
        let started = Instant::now();
        while active.load(Ordering::SeqCst) == 0 && started.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(active.load(Ordering::SeqCst), 1);

        first.abort();
        let _ = tauri::async_runtime::block_on(first);
        assert!(service
            .state()
            .providers
            .get("cancelled")
            .is_some_and(|state| state.refreshing));

        let retry = refresh_with_test_timeout(&service, "cancelled", true);
        assert!(!retry.refreshing);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(retry.error.is_none());
    }

    #[test]
    fn concurrent_forced_waiters_coalesce_one_follow_up() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SlowProvider {
            id: "lifecycle",
            calls: calls.clone(),
            active: active.clone(),
            maximum: maximum.clone(),
            delay: Duration::from_millis(80),
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::new(registry, storage));

        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let first_service = service.clone();
                let first = tauri::async_runtime::spawn(async move {
                    first_service.refresh("lifecycle", true).await
                });
                tokio::time::timeout(Duration::from_secs(1), async {
                    while active.load(Ordering::SeqCst) == 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("first refresh should start");
                let second_service = service.clone();
                let second = tauri::async_runtime::spawn(async move {
                    second_service.refresh("lifecycle", true).await
                });
                let third_service = service.clone();
                let third = tauri::async_runtime::spawn(async move {
                    third_service.refresh("lifecycle", true).await
                });
                assert!(first.await.unwrap().error.is_none());
                assert!(second.await.unwrap().error.is_none());
                assert!(third.await.unwrap().error.is_none());
            })
            .await
            .expect("coalesced refreshes should not deadlock");
        });

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn forced_refresh_after_credential_change_returns_authoritative_snapshot() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let credential = Arc::new(Mutex::new(Some("old".to_owned())));
        let calls = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CredentialProvider {
            id: "credential",
            credential: credential.clone(),
            calls: calls.clone(),
            active: active.clone(),
            maximum: maximum.clone(),
            delay: Duration::from_millis(60),
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::new(registry, storage));

        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let old_service = service.clone();
                let old_refresh = tauri::async_runtime::spawn(async move {
                    old_service.refresh("credential", true).await
                });
                tokio::time::timeout(Duration::from_secs(1), async {
                    while calls.load(Ordering::SeqCst) < 1 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("old-credential refresh should start");
                *credential.lock().unwrap() = Some("saved".to_owned());
                let saved = service.refresh("credential", true).await;
                assert_eq!(
                    saved.snapshot.and_then(|snapshot| snapshot.plan),
                    Some("saved".to_owned())
                );
                assert!(old_refresh.await.unwrap().error.is_none());

                let saved_service = service.clone();
                let saved_refresh = tauri::async_runtime::spawn(async move {
                    saved_service.refresh("credential", true).await
                });
                tokio::time::timeout(Duration::from_secs(1), async {
                    while calls.load(Ordering::SeqCst) < 3 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("saved-credential refresh should start");
                *credential.lock().unwrap() = None;
                let deleted = service.refresh("credential", true).await;
                assert_eq!(deleted.snapshot.and_then(|snapshot| snapshot.plan), None);
                assert!(saved_refresh.await.unwrap().error.is_none());
            })
            .await
            .expect("credential refreshes should not deadlock");
        });

        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn progress_reports_every_provider_completion() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let providers = [
            ("slow", Duration::from_millis(160)),
            ("fast", Duration::ZERO),
        ]
        .into_iter()
        .map(|(id, delay)| {
            Arc::new(SlowProvider {
                id,
                calls: calls.clone(),
                active: active.clone(),
                maximum: maximum.clone(),
                delay,
            }) as Arc<dyn UsageProvider>
        })
        .collect();
        let registry = Arc::new(ProviderRegistry::new(providers).unwrap());
        let service = Arc::new(ProviderService::with_refresh_timeout(
            registry,
            storage.clone(),
            Duration::from_millis(40),
        ));
        let observations = Arc::new(Mutex::new(Vec::new()));
        let observed = observations.clone();

        let final_state = tauri::async_runtime::block_on(service.refresh_enabled_with_progress(
            &["slow".into(), "fast".into()],
            true,
            move |state| {
                observed.lock().unwrap().push(state.clone());
            },
        ));

        let observations = observations.lock().unwrap();
        assert_eq!(observations.len(), 2);
        let completed = observations.last().unwrap();
        assert!(completed
            .providers
            .get("fast")
            .and_then(|state| state.snapshot.as_ref())
            .is_some());
        assert_eq!(
            completed
                .providers
                .get("slow")
                .and_then(|state| state.error.as_deref()),
            Some("Provider refresh timed out.")
        );
        assert!(storage.load_snapshot("fast").unwrap().is_some());
        assert_eq!(
            final_state
                .providers
                .get("slow")
                .and_then(|state| state.error.as_deref()),
            Some("Provider refresh timed out.")
        );
        drop(observations);

        let draining = Instant::now();
        while active.load(Ordering::SeqCst) != 0 && draining.elapsed() < Duration::from_secs(1) {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn failed_provider_is_backed_off_but_force_and_expiry_retry() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SequenceProvider {
            id: "failing",
            calls: calls.clone(),
            failures_before_success: usize::MAX,
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::new(registry, storage));

        tauri::async_runtime::block_on(service.refresh("failing", false));
        tauri::async_runtime::block_on(service.refresh("failing", false));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tauri::async_runtime::block_on(service.refresh("failing", true));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        service.last_failed_refresh.lock().unwrap().insert(
            "failing".into(),
            std::time::Instant::now()
                .checked_sub(FAILURE_RETRY_BACKOFF)
                .unwrap(),
        );
        tauri::async_runtime::block_on(service.refresh("failing", false));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn successful_retry_clears_failure_backoff() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(SequenceProvider {
            id: "recovering",
            calls,
            failures_before_success: 1,
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = Arc::new(ProviderService::new(registry, storage));

        tauri::async_runtime::block_on(service.refresh("recovering", false));
        assert!(service
            .last_failed_refresh
            .lock()
            .unwrap()
            .contains_key("recovering"));

        let recovered = tauri::async_runtime::block_on(service.refresh("recovering", true));
        assert!(recovered.error.is_none());
        assert!(!service
            .last_failed_refresh
            .lock()
            .unwrap()
            .contains_key("recovering"));
    }
}
