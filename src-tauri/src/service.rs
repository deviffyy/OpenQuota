use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
    active_workers: Arc<Mutex<HashSet<String>>>,
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
        }
        Self {
            registry,
            storage,
            states: RwLock::new(states),
            active_workers: Arc::new(Mutex::new(HashSet::new())),
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
        let Some(worker_lease) =
            RefreshWorkerLease::try_acquire(self.active_workers.clone(), provider_id)
        else {
            crate::app_debug!(
                "refresh",
                "single-flight skip {provider_id} (refresh worker already active)"
            );
            return self.provider_state(provider_id);
        };
        let started = Instant::now();
        let tag = format!("plugin:{provider_id}");
        crate::app_info!(&tag, "refresh start (force={force})");
        let previous_state = self.provider_state(provider_id);
        self.update_state(provider_id, |state| {
            state.refreshing = true;
            state.error = None;
            state.error_kind = None;
            state.last_attempt_at = Some(Utc::now());
        });
        let state_lease = worker_lease.clone();
        let mut worker = tauri::async_runtime::spawn_blocking(move || {
            let result = provider.refresh();
            (worker_lease, result)
        });
        let mut state_guard =
            RefreshStateGuard::new(self, provider_id, &previous_state, state_lease);
        let mut late_worker = None;
        let mut completed_worker_lease = None;
        let refresh_result = match tokio::time::timeout(self.refresh_timeout, &mut worker).await {
            Ok(Ok((worker_lease, result))) => {
                completed_worker_lease = Some(worker_lease);
                result
            }
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
            .and_then(|snapshot| validate_snapshot(&self.registry, provider_id, snapshot));
        match &refresh_result {
            Ok(_) => crate::app_info!(&tag, "refresh end ({}ms)", started.elapsed().as_millis()),
            Err(error) => crate::app_warn!(
                &tag,
                "refresh failed ({}ms, kind={:?}): {error}",
                started.elapsed().as_millis(),
                error.kind()
            ),
        }
        let state = self.apply_refresh_result(provider_id, refresh_result);
        if state.error.is_none() {
            if let Ok(mut last) = self.last_live_refresh.lock() {
                last.insert(provider_id.to_owned(), Instant::now());
            }
            if let Ok(mut failures) = self.last_failed_refresh.lock() {
                failures.remove(provider_id);
            }
        } else if let Ok(mut failures) = self.last_failed_refresh.lock() {
            failures.insert(provider_id.to_owned(), Instant::now());
        }
        state_guard.disarm();
        drop(completed_worker_lease);
        if let Some(worker) = late_worker {
            let late_tag = tag.clone();
            tauri::async_runtime::spawn(async move {
                match worker.await {
                    Ok((worker_lease, _)) => {
                        crate::app_debug!(
                            &late_tag,
                            "timed-out refresh worker finished; discarded late result"
                        );
                        drop(worker_lease);
                    }
                    Err(_) => crate::app_debug!(
                        &late_tag,
                        "timed-out refresh worker stopped; discarded late failure"
                    ),
                }
            });
        }
        state
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

#[derive(Clone)]
struct RefreshWorkerLease {
    _inner: Arc<RefreshWorkerLeaseInner>,
}

struct RefreshWorkerLeaseInner {
    provider_id: String,
    active_workers: Arc<Mutex<HashSet<String>>>,
}

struct RefreshStateGuard<'a> {
    service: &'a ProviderService,
    provider_id: String,
    previous_error: Option<String>,
    previous_error_kind: Option<ProviderErrorKind>,
    worker_lease: Option<RefreshWorkerLease>,
    armed: bool,
}

impl<'a> RefreshStateGuard<'a> {
    fn new(
        service: &'a ProviderService,
        provider_id: &str,
        previous_state: &ProviderViewState,
        worker_lease: RefreshWorkerLease,
    ) -> Self {
        Self {
            service,
            provider_id: provider_id.to_owned(),
            previous_error: previous_state.error.clone(),
            previous_error_kind: previous_state.error_kind,
            worker_lease: Some(worker_lease),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
        self.worker_lease.take();
    }
}

impl Drop for RefreshStateGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.service.update_state(&self.provider_id, |state| {
            state.refreshing = false;
            state.error = self.previous_error.clone();
            state.error_kind = self.previous_error_kind;
        });
        self.worker_lease.take();
    }
}

impl RefreshWorkerLease {
    fn try_acquire(active_workers: Arc<Mutex<HashSet<String>>>, provider_id: &str) -> Option<Self> {
        let mut active = active_workers.lock().ok()?;
        if !active.insert(provider_id.to_owned()) {
            return None;
        }
        drop(active);
        Some(Self {
            _inner: Arc::new(RefreshWorkerLeaseInner {
                provider_id: provider_id.to_owned(),
                active_workers,
            }),
        })
    }
}

impl Drop for RefreshWorkerLeaseInner {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active_workers.lock() {
            active.remove(&self.provider_id);
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
            Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    use chrono::Utc;
    use tempfile::tempdir;

    use super::{
        merge_refresh_result, validate_snapshot, ProviderService, RefreshStateGuard,
        RefreshWorkerLease,
    };
    use crate::{
        models::{
            MetricDefinition, MetricSection, MetricSource, ProviderDefinition, ProviderErrorKind,
            ProviderSnapshot, ProviderViewState, QuotaFormat, QuotaWindow, StatusMetric,
            StatusTone, UsageHistory,
        },
        policy::FAILURE_RETRY_BACKOFF,
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

    #[test]
    fn failed_refresh_preserves_last_successful_snapshot() {
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
        assert!(state.stale);
        assert_eq!(state.error.as_deref(), Some("offline"));
        assert_eq!(state.error_kind, Some(ProviderErrorKind::Network));
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
            calls,
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

        let started = Instant::now();
        let timed_out = tauri::async_runtime::block_on(service.refresh("slow", true));
        assert!(started.elapsed() < Duration::from_millis(120));
        assert_eq!(
            timed_out.error.as_deref(),
            Some("Provider refresh timed out.")
        );
        assert_eq!(timed_out.error_kind, Some(ProviderErrorKind::Network));
        assert!(timed_out.stale);
        assert_eq!(
            timed_out
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.plan.as_deref()),
            Some("cached")
        );

        let duplicate_started = Instant::now();
        let duplicate = tauri::async_runtime::block_on(service.refresh("slow", true));
        assert!(duplicate_started.elapsed() < Duration::from_millis(100));
        assert_eq!(duplicate.error, timed_out.error);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);

        thread::sleep(Duration::from_millis(180));
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(!service.active_workers.lock().unwrap().contains("slow"));
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
        assert!(!service
            .state()
            .providers
            .get("cancelled")
            .is_some_and(|state| state.refreshing));

        let retry = tauri::async_runtime::block_on(service.refresh("cancelled", true));
        assert!(!retry.refreshing);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);

        let draining = Instant::now();
        while service.active_workers.lock().unwrap().contains("cancelled")
            && draining.elapsed() < Duration::from_secs(1)
        {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(!service.active_workers.lock().unwrap().contains("cancelled"));

        let completed = tauri::async_runtime::block_on(service.refresh("cancelled", true));
        assert!(completed.error.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn completed_worker_output_keeps_single_flight_until_state_cleanup() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
        let provider = Arc::new(SlowProvider {
            id: "lifecycle",
            calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            maximum: Arc::new(AtomicUsize::new(0)),
            delay: Duration::ZERO,
        }) as Arc<dyn UsageProvider>;
        let registry = Arc::new(ProviderRegistry::new(vec![provider]).unwrap());
        let service = ProviderService::new(registry, storage);

        let worker_lease =
            RefreshWorkerLease::try_acquire(service.active_workers.clone(), "lifecycle").unwrap();
        let state_lease = worker_lease.clone();
        let previous_state = service.provider_state("lifecycle");
        service.update_state("lifecycle", |state| state.refreshing = true);
        let state_guard =
            RefreshStateGuard::new(&service, "lifecycle", &previous_state, state_lease);
        let completed_output = (
            worker_lease,
            Ok::<ProviderSnapshot, ProviderError>(test_snapshot("lifecycle")),
        );

        drop(completed_output);
        assert!(service.active_workers.lock().unwrap().contains("lifecycle"));
        assert!(
            RefreshWorkerLease::try_acquire(service.active_workers.clone(), "lifecycle").is_none()
        );

        drop(state_guard);
        assert!(!service.active_workers.lock().unwrap().contains("lifecycle"));
        assert!(!service.provider_state("lifecycle").refreshing);

        let next_lease =
            RefreshWorkerLease::try_acquire(service.active_workers.clone(), "lifecycle").unwrap();
        service.update_state("lifecycle", |state| state.refreshing = true);
        assert!(service.provider_state("lifecycle").refreshing);
        drop(next_lease);
    }

    #[test]
    fn progress_reports_fast_provider_before_slow_provider_deadline() {
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
        let started = Instant::now();

        let final_state = tauri::async_runtime::block_on(service.refresh_enabled_with_progress(
            &["slow".into(), "fast".into()],
            true,
            move |state| {
                observed
                    .lock()
                    .unwrap()
                    .push((started.elapsed(), state.clone()));
            },
        ));

        let observations = observations.lock().unwrap();
        assert_eq!(observations.len(), 2);
        assert!(observations[0].0 < Duration::from_millis(120));
        assert!(observations[0]
            .1
            .providers
            .get("fast")
            .and_then(|state| state.snapshot.as_ref())
            .is_some());
        assert!(observations[0]
            .1
            .providers
            .get("slow")
            .and_then(|state| state.error.as_ref())
            .is_none());
        assert!(storage.load_snapshot("fast").unwrap().is_some());
        assert_eq!(
            final_state
                .providers
                .get("slow")
                .and_then(|state| state.error.as_deref()),
            Some("Provider refresh timed out.")
        );
        drop(observations);

        thread::sleep(Duration::from_millis(180));
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
        let service = ProviderService::new(registry, storage);

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
        let service = ProviderService::new(registry, storage);

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
