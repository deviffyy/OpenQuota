use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::{blocking::Client, header::ETAG, StatusCode};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

use super::{
    codecs::{
        catalog_from_compact, catalog_from_litellm, catalog_from_models_dev, compact_data,
        PricingCodecError,
    },
    ModelPricing, PricingCatalog, PricingSupplement,
};

const REFRESH_INTERVAL: TimeDelta = TimeDelta::hours(24);
const FAILURE_RETRY_INTERVAL: TimeDelta = TimeDelta::minutes(30);

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const SUPPLEMENT_URL: &str = "https://raw.githubusercontent.com/deviffyy/OpenQuota/main/src-tauri/resources/pricing_supplement.json";

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceId {
    Litellm,
    ModelsDev,
    Supplement,
}

impl SourceId {
    const ALL: [Self; 3] = [Self::Litellm, Self::ModelsDev, Self::Supplement];

    fn file_name(self) -> &'static str {
        match self {
            Self::Litellm => "litellm.json",
            Self::ModelsDev => "models_dev.json",
            Self::Supplement => "supplement.json",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::Litellm => LITELLM_URL,
            Self::ModelsDev => MODELS_DEV_URL,
            Self::Supplement => SUPPLEMENT_URL,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SourceState {
    etag: Option<String>,
    fetched_at: Option<DateTime<Utc>>,
    failed_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
struct BundledSources {
    supplement: Arc<[u8]>,
    litellm: Arc<[u8]>,
    models_dev: Arc<[u8]>,
}

impl Default for BundledSources {
    fn default() -> Self {
        Self {
            supplement: Arc::from(
                include_bytes!("../../resources/pricing_supplement.json").as_slice(),
            ),
            litellm: Arc::from(
                include_bytes!("../../resources/pricing_litellm_snapshot.json").as_slice(),
            ),
            models_dev: Arc::from(
                include_bytes!("../../resources/pricing_models_dev_snapshot.json").as_slice(),
            ),
        }
    }
}

struct FetchResponse {
    status: StatusCode,
    etag: Option<String>,
    body: Vec<u8>,
}

trait PricingHttpClient: Send + Sync {
    fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchResponse, PricingStoreError>;
}

struct ReqwestPricingHttpClient {
    client: Client,
}

impl ReqwestPricingHttpClient {
    fn new() -> Result<Self, PricingStoreError> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("OpenQuota pricing refresh")
                .build()?,
        })
    }
}

impl PricingHttpClient for ReqwestPricingHttpClient {
    fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchResponse, PricingStoreError> {
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        let response = request.send()?;
        let status = response.status();
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = if status == StatusCode::NOT_MODIFIED {
            Vec::new()
        } else {
            response.bytes()?.to_vec()
        };
        Ok(FetchResponse { status, etag, body })
    }
}

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

pub struct PricingStore {
    cache_directory: PathBuf,
    bundled: BundledSources,
    http: Arc<dyn PricingHttpClient>,
    now: Clock,
    pricing: RwLock<Arc<ModelPricing>>,
    source_states: Mutex<HashMap<SourceId, SourceState>>,
    refresh_lock: Mutex<()>,
    refresh_in_flight: AtomicBool,
}

struct RefreshCompletion<'a>(&'a AtomicBool);

impl Drop for RefreshCompletion<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl PricingStore {
    pub fn new(cache_directory: PathBuf) -> Result<Self, PricingStoreError> {
        Self::with_dependencies(
            cache_directory,
            BundledSources::default(),
            Arc::new(ReqwestPricingHttpClient::new()?),
            Arc::new(Utc::now),
        )
    }

    fn with_dependencies(
        cache_directory: PathBuf,
        bundled: BundledSources,
        http: Arc<dyn PricingHttpClient>,
        now: Clock,
    ) -> Result<Self, PricingStoreError> {
        let initial = load_pricing(&cache_directory, &bundled)?;
        let source_states = read_states(&cache_directory);
        Ok(Self {
            cache_directory,
            bundled,
            http,
            now,
            pricing: RwLock::new(Arc::new(initial)),
            source_states: Mutex::new(source_states),
            refresh_lock: Mutex::new(()),
            refresh_in_flight: AtomicBool::new(false),
        })
    }

    /// Returns the current immutable snapshot immediately. If a source is stale, one background
    /// refresh is started for the next call; callers never wait for disk or network I/O.
    pub fn current(self: &Arc<Self>) -> Arc<ModelPricing> {
        let snapshot = self.snapshot();
        if self.has_due_source()
            && self
                .refresh_in_flight
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            let store = Arc::clone(self);
            std::thread::spawn(move || {
                let _completion = RefreshCompletion(&store.refresh_in_flight);
                store.refresh_due();
            });
        }
        snapshot
    }

    fn snapshot(&self) -> Arc<ModelPricing> {
        self.pricing
            .read()
            .map(|pricing| pricing.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    fn has_due_source(&self) -> bool {
        let now = (self.now)();
        let states = self
            .source_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        SourceId::ALL
            .iter()
            .any(|source| states.get(source).is_none_or(|state| is_due(state, now)))
    }

    fn refresh_due(&self) {
        let Ok(_guard) = self.refresh_lock.try_lock() else {
            return;
        };
        let mut states = self
            .source_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let now = (self.now)();
        let mut changed = false;
        for source in SourceId::ALL {
            let state = states.entry(source).or_default();
            if !is_due(state, now) {
                continue;
            }
            match self.fetch_source(source, state, now) {
                Ok(source_changed) => changed |= source_changed,
                Err(error) => {
                    state.failed_at = Some(now);
                    eprintln!(
                        "pricing {} refresh failed, keeping cached data: {error}",
                        source.file_name()
                    );
                }
            }
        }
        *self
            .source_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = states.clone();
        if changed {
            match load_pricing(&self.cache_directory, &self.bundled) {
                Ok(pricing) => {
                    let mut current = self
                        .pricing
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *current = Arc::new(pricing);
                }
                Err(error) => {
                    eprintln!("pricing rebuild failed, keeping current snapshot: {error}")
                }
            }
        }
        if let Err(error) = write_json_atomic(&state_file(&self.cache_directory), &states) {
            eprintln!("pricing fetch state could not be persisted: {error}");
        }
    }

    fn fetch_source(
        &self,
        source: SourceId,
        state: &mut SourceState,
        now: DateTime<Utc>,
    ) -> Result<bool, PricingStoreError> {
        let response = self.http.fetch(source.url(), state.etag.as_deref())?;
        match response.status {
            StatusCode::OK => {
                let cache_data = validated_cache_data(source, &response.body)?;
                write_bytes_atomic(&self.cache_file(source), &cache_data)?;
                state.etag = response.etag;
                state.fetched_at = Some(now);
                state.failed_at = None;
                Ok(true)
            }
            StatusCode::NOT_MODIFIED => {
                state.fetched_at = Some(now);
                state.failed_at = None;
                Ok(false)
            }
            status => Err(PricingStoreError::HttpStatus(status.as_u16())),
        }
    }

    fn cache_file(&self, source: SourceId) -> PathBuf {
        self.cache_directory.join(source.file_name())
    }
}

fn is_due(state: &SourceState, now: DateTime<Utc>) -> bool {
    if state
        .failed_at
        .is_some_and(|failed| now - failed < FAILURE_RETRY_INTERVAL)
    {
        return false;
    }
    state
        .fetched_at
        .is_none_or(|fetched| now - fetched >= REFRESH_INTERVAL)
}

fn load_pricing(
    cache_directory: &Path,
    bundled: &BundledSources,
) -> Result<ModelPricing, PricingStoreError> {
    let supplement = match fs::read(cache_directory.join(SourceId::Supplement.file_name())) {
        Ok(cached) => match PricingSupplement::decode(&cached) {
            Ok(supplement) => supplement,
            Err(error) => {
                eprintln!("cached pricing supplement unreadable, using bundled: {error}");
                PricingSupplement::decode(&bundled.supplement)?
            }
        },
        Err(_) => PricingSupplement::decode(&bundled.supplement)?,
    };
    let _supplement_updated_at = supplement.updated_at.as_deref();
    let primary = load_catalog(cache_directory, SourceId::Litellm, &bundled.litellm)?;
    let secondary = load_catalog(cache_directory, SourceId::ModelsDev, &bundled.models_dev)?;
    Ok(ModelPricing::new(supplement, primary, secondary))
}

fn load_catalog(
    cache_directory: &Path,
    source: SourceId,
    bundled: &[u8],
) -> Result<PricingCatalog, PricingStoreError> {
    let mut catalog = catalog_from_compact(bundled)?;
    if let Ok(cached) = fs::read(cache_directory.join(source.file_name())) {
        match catalog_from_compact(&cached) {
            Ok(cache) => catalog = catalog.merging(cache),
            Err(error) => eprintln!(
                "cached {} catalog unreadable, using bundled: {error}",
                source.file_name()
            ),
        }
    }
    Ok(catalog)
}

fn validated_cache_data(source: SourceId, body: &[u8]) -> Result<Vec<u8>, PricingStoreError> {
    match source {
        SourceId::Litellm => Ok(compact_data(&catalog_from_litellm(body)?)?),
        SourceId::ModelsDev => Ok(compact_data(&catalog_from_models_dev(body)?)?),
        SourceId::Supplement => {
            PricingSupplement::decode(body)?;
            Ok(body.to_vec())
        }
    }
}

fn state_file(cache_directory: &Path) -> PathBuf {
    cache_directory.join("state.json")
}

fn read_states(cache_directory: &Path) -> HashMap<SourceId, SourceState> {
    fs::read(state_file(cache_directory))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), PricingStoreError> {
    write_bytes_atomic(path, &serde_json::to_vec(value)?)
}

fn write_bytes_atomic(path: &Path, data: &[u8]) -> Result<(), PricingStoreError> {
    let parent = path.parent().ok_or(PricingStoreError::MissingCacheParent)?;
    fs::create_dir_all(parent)?;
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(data)?;
    file.flush()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum PricingStoreError {
    #[error("Pricing cache path has no parent directory.")]
    MissingCacheParent,
    #[error("Pricing cache could not be read or written.")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Codec(#[from] PricingCodecError),
    #[error(transparent)]
    Supplement(#[from] super::supplement::SupplementError),
    #[error("Pricing request failed.")]
    Request(#[from] reqwest::Error),
    #[error("Pricing endpoint returned HTTP {0}.")]
    HttpStatus(u16),
    #[error("Pricing state is invalid JSON.")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use chrono::TimeZone;
    use tempfile::tempdir;

    use super::*;

    #[derive(Default)]
    struct StubHttp {
        responses: Mutex<VecDeque<Result<FetchResponse, u16>>>,
        requests: Mutex<Vec<(String, Option<String>)>>,
    }

    impl StubHttp {
        fn push(&self, response: FetchResponse) {
            self.responses.lock().unwrap().push_back(Ok(response));
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    impl PricingHttpClient for StubHttp {
        fn fetch(&self, url: &str, etag: Option<&str>) -> Result<FetchResponse, PricingStoreError> {
            self.requests
                .lock()
                .unwrap()
                .push((url.to_owned(), etag.map(str::to_owned)));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(500))
                .map_err(PricingStoreError::HttpStatus)
        }
    }

    fn bundled() -> BundledSources {
        BundledSources {
            supplement: Arc::from(
                br#"{"pricing":{"auto":{"input_per_million":1.25,"output_per_million":6}},"fast_multipliers":{},"alias_rules":[]}"#
                    .as_slice(),
            ),
            litellm: Arc::from(
                br#"{"models":{"bundled-model":{"i":1,"o":2,"cw":1,"cr":0.1},"snapshot-only":{"i":2,"o":3,"cw":2,"cr":0.2}}}"#
                    .as_slice(),
            ),
            models_dev: Arc::from(
                br#"{"models":{"bundled-dev-model":{"i":3,"o":4,"cw":3,"cr":0.3}}}"#
                    .as_slice(),
            ),
        }
    }

    fn response(body: &[u8]) -> FetchResponse {
        FetchResponse {
            status: StatusCode::OK,
            etag: Some("\"v1\"".into()),
            body: body.to_vec(),
        }
    }

    #[test]
    fn serves_bundled_data_before_fetch() {
        let directory = tempdir().unwrap();
        let store = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            Arc::new(StubHttp::default()),
            Arc::new(Utc::now),
        )
        .unwrap();
        assert_eq!(
            store
                .snapshot()
                .resolve("bundled-model")
                .unwrap()
                .input_per_million,
            1.0
        );
        assert_eq!(
            store
                .snapshot()
                .resolve("bundled-dev-model")
                .unwrap()
                .input_per_million,
            3.0
        );
        assert_eq!(
            store.snapshot().resolve("auto").unwrap().input_per_million,
            1.25
        );
    }

    #[test]
    fn refreshes_all_sources_and_persists_cache() {
        let directory = tempdir().unwrap();
        let http = Arc::new(StubHttp::default());
        http.push(response(br#"{"fetched-model":{"input_cost_per_token":0.000005,"output_cost_per_token":0.00001},"bundled-model":{"input_cost_per_token":0.000007,"output_cost_per_token":0.00001}}"#));
        http.push(response(
            br#"{"xai":{"models":{"fetched-dev":{"cost":{"input":1,"output":2}}}}}"#,
        ));
        http.push(response(br#"{"pricing":{"auto":{"input_per_million":9,"output_per_million":9}},"fast_multipliers":{},"alias_rules":[]}"#));
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let store = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            http.clone(),
            Arc::new(move || now),
        )
        .unwrap();
        store.refresh_due();
        assert_eq!(http.request_count(), 3);
        store.refresh_due();
        assert_eq!(
            http.request_count(),
            3,
            "fresh sources must respect the 24-hour TTL"
        );
        assert_eq!(
            store
                .snapshot()
                .resolve("fetched-model")
                .unwrap()
                .input_per_million,
            5.0
        );
        assert_eq!(
            store
                .snapshot()
                .resolve("fetched-dev")
                .unwrap()
                .input_per_million,
            1.0
        );
        assert_eq!(
            store
                .snapshot()
                .resolve("snapshot-only")
                .unwrap()
                .input_per_million,
            2.0
        );
        assert_eq!(
            store.snapshot().resolve("auto").unwrap().input_per_million,
            9.0
        );
        assert_eq!(
            store
                .snapshot()
                .resolve("bundled-model")
                .unwrap()
                .input_per_million,
            7.0
        );
    }

    #[test]
    fn current_returns_immediately_and_coalesces_background_refresh() {
        let directory = tempdir().unwrap();
        let http = Arc::new(StubHttp::default());
        http.push(response(br#"{"fetched-model":{"input_cost_per_token":0.000005,"output_cost_per_token":0.00001}}"#));
        http.push(response(
            br#"{"x":{"models":{"fetched-dev":{"cost":{"input":1,"output":2}}}}}"#,
        ));
        http.push(response(br#"{"pricing":{"auto":{"input_per_million":9,"output_per_million":9}},"fast_multipliers":{},"alias_rules":[]}"#));
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let store = Arc::new(
            PricingStore::with_dependencies(
                directory.path().to_path_buf(),
                bundled(),
                http.clone(),
                Arc::new(move || now),
            )
            .unwrap(),
        );

        let original = store.current();
        for _ in 0..10 {
            let _ = store.current();
        }
        assert!(original.resolve("fetched-model").is_none());

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while store.refresh_in_flight.load(Ordering::Acquire)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!store.refresh_in_flight.load(Ordering::Acquire));
        assert_eq!(http.request_count(), 3, "only one refresh task may run");
        assert_eq!(
            store
                .snapshot()
                .resolve("fetched-model")
                .unwrap()
                .input_per_million,
            5.0
        );
    }

    #[test]
    fn etag_304_and_failed_refresh_keep_good_cache() {
        let directory = tempdir().unwrap();
        let first_http = Arc::new(StubHttp::default());
        first_http.push(response(br#"{"fetched-model":{"input_cost_per_token":0.000005,"output_cost_per_token":0.00001}}"#));
        first_http.push(response(
            br#"{"x":{"models":{"fetched-dev":{"cost":{"input":1,"output":2}}}}}"#,
        ));
        first_http.push(response(br#"{"pricing":{"auto":{"input_per_million":9,"output_per_million":9}},"fast_multipliers":{},"alias_rules":[]}"#));
        let first_now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let first = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            first_http,
            Arc::new(move || first_now),
        )
        .unwrap();
        first.refresh_due();

        let second_http = Arc::new(StubHttp::default());
        for _ in 0..3 {
            second_http.push(FetchResponse {
                status: StatusCode::NOT_MODIFIED,
                etag: None,
                body: Vec::new(),
            });
        }
        let later = first_now + TimeDelta::hours(25);
        let second = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            second_http.clone(),
            Arc::new(move || later),
        )
        .unwrap();
        second.refresh_due();
        assert!(second_http
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(_, etag)| etag.as_deref() == Some("\"v1\"")));
        assert_eq!(
            second
                .snapshot()
                .resolve("fetched-model")
                .unwrap()
                .input_per_million,
            5.0
        );

        let failed_http = Arc::new(StubHttp::default());
        let failed_later = later + TimeDelta::hours(25);
        let failed = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            failed_http.clone(),
            Arc::new(move || failed_later),
        )
        .unwrap();
        failed.refresh_due();
        failed.refresh_due();
        assert_eq!(
            failed_http.request_count(),
            3,
            "failure backoff prevents immediate retry"
        );
        assert_eq!(
            failed
                .snapshot()
                .resolve("fetched-model")
                .unwrap()
                .input_per_million,
            5.0
        );
    }

    #[test]
    fn garbage_feed_never_replaces_good_cache() {
        let directory = tempdir().unwrap();
        let good_http = Arc::new(StubHttp::default());
        good_http.push(response(br#"{"fetched-model":{"input_cost_per_token":0.000005,"output_cost_per_token":0.00001}}"#));
        good_http.push(response(
            br#"{"x":{"models":{"fetched-dev":{"cost":{"input":1,"output":2}}}}}"#,
        ));
        good_http.push(response(br#"{"pricing":{"auto":{"input_per_million":9,"output_per_million":9}},"alias_rules":[]}"#));
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        let good = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            good_http,
            Arc::new(move || now),
        )
        .unwrap();
        good.refresh_due();

        let garbage_http = Arc::new(StubHttp::default());
        for _ in 0..3 {
            garbage_http.push(response(b"not json"));
        }
        let later = now + TimeDelta::hours(25);
        let garbage = PricingStore::with_dependencies(
            directory.path().to_path_buf(),
            bundled(),
            garbage_http,
            Arc::new(move || later),
        )
        .unwrap();
        garbage.refresh_due();
        assert_eq!(
            garbage
                .snapshot()
                .resolve("fetched-model")
                .unwrap()
                .input_per_million,
            5.0
        );
    }
}
