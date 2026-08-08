mod auth;
mod client;
mod mapper;

use std::{
    ops::ControlFlow,
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::Utc;
use reqwest::StatusCode;
use thiserror::Error;

use crate::models::{
    MetricDefinition, MetricSection, ProviderDefinition, ProviderErrorKind, ProviderLink,
    ProviderSnapshot, UsageHistory, ValueMetric,
};

use self::{
    auth::CopilotAuthStore,
    client::{CopilotClient, CopilotResponse},
    mapper::{map_org_usage, map_usage, org_logins},
};

use super::{ProviderError, UsageProvider};

const ORG_LOOKUP_BUDGET: Duration = Duration::from_secs(20);
const ORG_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MIN_ORG_REQUEST_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "copilot".into(),
        display_name: "Copilot".into(),
        short_name: "Co".into(),
        fallback_enabled: false,
        local_usage_source_note: None,
        local_usage_source_key: None,
        pi_usage_source_key: None,
        links: vec![
            ProviderLink::new("Status", "https://www.githubstatus.com/"),
            ProviderLink::new("Dashboard", "https://github.com/settings/billing"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "copilot.premium",
                "Credits",
                "premium",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "C",
            ),
            MetricDefinition::value(
                "copilot.extra",
                "Extra Usage",
                "extra",
                true,
                MetricSection::AlwaysVisible,
                false,
                "E",
                None,
            ),
            MetricDefinition::value(
                "copilot.orgCredits",
                "Org Credits",
                "orgCredits",
                true,
                MetricSection::OnDemand,
                false,
                "OC",
                None,
            ),
            MetricDefinition::value(
                "copilot.orgSpend",
                "Org Spend",
                "orgSpend",
                true,
                MetricSection::OnDemand,
                false,
                "OS",
                None,
            ),
            MetricDefinition::quota(
                "copilot.chat",
                "Chat",
                "chat",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "Ch",
            ),
            MetricDefinition::quota(
                "copilot.completions",
                "Completions",
                "completions",
                false,
                true,
                MetricSection::OnDemand,
                false,
                "Cm",
            ),
        ],
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(super) enum CopilotError {
    #[error("Sign in to GitHub Copilot in your editor, or run `gh auth login`, and try again.")]
    NotLoggedIn,
    #[error("Your GitHub token is invalid or expired. Run `gh auth login` and try again.")]
    InvalidToken,
    #[error("Could not reach GitHub. Check your internet connection.")]
    ConnectionFailed,
    #[error("Copilot usage data is temporarily unavailable.")]
    InvalidResponse,
    #[error("Copilot usage request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("Copilot usage data is unavailable for this account.")]
    QuotaUnavailable,
}

impl From<CopilotError> for ProviderError {
    fn from(error: CopilotError) -> Self {
        let kind = match error {
            CopilotError::NotLoggedIn | CopilotError::InvalidToken => {
                ProviderErrorKind::Authentication
            }
            CopilotError::ConnectionFailed => ProviderErrorKind::Network,
            CopilotError::RequestFailed(429) => ProviderErrorKind::RateLimited,
            CopilotError::RequestFailed(401 | 403) => ProviderErrorKind::Authentication,
            CopilotError::RequestFailed(status) if status >= 500 => ProviderErrorKind::Network,
            CopilotError::InvalidResponse | CopilotError::RequestFailed(_) => {
                ProviderErrorKind::InvalidResponse
            }
            CopilotError::QuotaUnavailable => ProviderErrorKind::Permission,
        };
        ProviderError::new(kind, error.to_string())
    }
}

pub struct CopilotProvider {
    auth: CopilotAuthStore,
    client: CopilotClient,
    cached_org: Mutex<Option<String>>,
}

impl CopilotProvider {
    pub fn new() -> Result<Self, ProviderError> {
        Ok(Self {
            auth: CopilotAuthStore::new(),
            client: CopilotClient::new().map_err(ProviderError::from)?,
            cached_org: Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn with_dependencies(auth: CopilotAuthStore, client: CopilotClient) -> Self {
        Self {
            auth,
            client,
            cached_org: Mutex::new(None),
        }
    }

    fn refresh_inner(&self) -> Result<ProviderSnapshot, CopilotError> {
        let mut saw_auth_failure = false;
        self.auth
            .visit_candidates(|token| {
                let response = match self.client.fetch_usage(token.as_str()) {
                    Ok(response) => response,
                    Err(error) => return ControlFlow::Break(Err(error)),
                };
                match require_usage_success(&response) {
                    Ok(()) => {}
                    Err(CopilotError::InvalidToken) => {
                        saw_auth_failure = true;
                        return ControlFlow::Continue(());
                    }
                    Err(error) => return ControlFlow::Break(Err(error)),
                }
                let mut mapped = match map_usage(&response.body) {
                    Ok(mapped) => mapped,
                    Err(error) => return ControlFlow::Break(Err(error)),
                };
                if mapped.is_org_managed_seat {
                    mapped.value_metrics = self.org_billing_metrics(token.as_str());
                }
                ControlFlow::Break(Ok(ProviderSnapshot {
                    provider_id: "copilot".into(),
                    plan: mapped.plan,
                    quotas: mapped.quotas,
                    value_metrics: mapped.value_metrics,
                    status_metrics: Vec::new(),
                    notices: Vec::new(),
                    usage: UsageHistory::default(),
                    warnings: Vec::new(),
                    refreshed_at: Utc::now(),
                }))
            })
            .unwrap_or({
                Err(if saw_auth_failure {
                    CopilotError::InvalidToken
                } else {
                    CopilotError::NotLoggedIn
                })
            })
    }

    fn org_billing_metrics(&self, token: &str) -> Vec<ValueMetric> {
        let started = Instant::now();
        let cached = self.cached_org.lock().ok().and_then(|value| value.clone());
        if let Some(org) = cached {
            let Some(timeout) = org_request_timeout(started) else {
                return Vec::new();
            };
            match self.org_usage(&org, token, timeout) {
                OrgUsageOutcome::Metrics(metrics) => return metrics,
                OrgUsageOutcome::Transient => {
                    crate::app_warn!(
                        "provider:copilot",
                        "remembered organization billing is temporarily unavailable"
                    );
                    return Vec::new();
                }
                OrgUsageOutcome::NoUsage => {
                    if let Ok(mut cached) = self.cached_org.lock() {
                        *cached = None;
                    }
                }
            }
        }

        let Some(timeout) = org_request_timeout(started) else {
            return Vec::new();
        };
        let response = match self.client.fetch_orgs(token, timeout) {
            Ok(response) => response,
            Err(_) => {
                crate::app_warn!(
                    "provider:copilot",
                    "organization list is temporarily unavailable"
                );
                return Vec::new();
            }
        };
        if response.status != StatusCode::OK {
            crate::app_debug!(
                "provider:copilot",
                "organization list HTTP {}; skipping billing lookup",
                response.status.as_u16()
            );
            return Vec::new();
        }

        for org in org_logins(&response.body) {
            let Some(timeout) = org_request_timeout(started) else {
                crate::app_warn!(
                    "provider:copilot",
                    "organization billing lookup reached its time budget"
                );
                break;
            };
            match self.org_usage(&org, token, timeout) {
                OrgUsageOutcome::Metrics(metrics) => {
                    if let Ok(mut cached) = self.cached_org.lock() {
                        *cached = Some(org);
                    }
                    return metrics;
                }
                OrgUsageOutcome::NoUsage | OrgUsageOutcome::Transient => continue,
            }
        }
        Vec::new()
    }

    fn org_usage(&self, org: &str, token: &str, timeout: Duration) -> OrgUsageOutcome {
        let response = match self.client.fetch_org_usage(org, token, timeout) {
            Ok(response) => response,
            Err(_) => return OrgUsageOutcome::Transient,
        };
        if response.status == StatusCode::OK {
            return map_org_usage(&response.body)
                .map(OrgUsageOutcome::Metrics)
                .unwrap_or(OrgUsageOutcome::NoUsage);
        }
        crate::app_debug!(
            "provider:copilot",
            "organization billing HTTP {}",
            response.status.as_u16()
        );
        if response.status == StatusCode::TOO_MANY_REQUESTS || response.status.is_server_error() {
            OrgUsageOutcome::Transient
        } else {
            OrgUsageOutcome::NoUsage
        }
    }
}

impl UsageProvider for CopilotProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        self.auth.has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError> {
        self.refresh_inner().map_err(ProviderError::from)
    }
}

enum OrgUsageOutcome {
    Metrics(Vec<ValueMetric>),
    NoUsage,
    Transient,
}

fn org_request_timeout(started: Instant) -> Option<Duration> {
    ORG_LOOKUP_BUDGET
        .checked_sub(started.elapsed())
        .filter(|remaining| *remaining >= MIN_ORG_REQUEST_TIMEOUT)
        .map(|remaining| remaining.min(ORG_REQUEST_TIMEOUT))
}

fn require_usage_success(response: &CopilotResponse) -> Result<(), CopilotError> {
    if matches!(
        response.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err(CopilotError::InvalidToken);
    }
    if !response.status.is_success() {
        return Err(CopilotError::RequestFailed(response.status.as_u16()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Shutdown, TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use serde_json::{json, Value};

    use crate::{
        models::{MetricSection, MetricValueKind, ProviderErrorKind, QuotaFormat},
        providers::UsageProvider,
    };

    use super::{
        auth::CopilotAuthStore, client::CopilotClient, definition, org_request_timeout,
        CopilotProvider, ORG_LOOKUP_BUDGET, ORG_REQUEST_TIMEOUT,
    };

    struct Route {
        path: String,
        status: u16,
        body: String,
    }

    struct TestServer {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        worker: Option<thread::JoinHandle<Result<(), String>>>,
    }

    impl TestServer {
        fn finish(mut self) -> Arc<Mutex<Vec<String>>> {
            let result = self
                .worker
                .take()
                .expect("test server worker should exist")
                .join()
                .expect("test server worker should not panic");
            result.expect("test server should serve every expected request");
            self.requests.clone()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn accept_before(listener: &TcpListener, deadline: Instant) -> Result<TcpStream, String> {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).map_err(|error| {
                        format!("could not configure test HTTP stream: {error}")
                    })?;
                    return Ok(stream);
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Err("timed out waiting for a test HTTP request".into());
                }
                Err(error) => return Err(format!("could not accept test HTTP request: {error}")),
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> Result<Option<String>, String> {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|error| format!("could not configure test HTTP stream: {error}"))?;
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0_u8; 1024];
            let count = stream
                .read(&mut chunk)
                .map_err(|error| format!("could not read test HTTP request: {error}"))?;
            if count == 0 {
                return Ok(None);
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
            }
        }
    }

    fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), String> {
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|error| format!("could not write test HTTP response: {error}"))?;
        stream
            .shutdown(Shutdown::Write)
            .map_err(|error| format!("could not finish test HTTP response: {error}"))
    }

    fn routing_server(routes: Vec<Route>, expected: usize) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while captured.lock().unwrap().len() < expected && Instant::now() < deadline {
                let mut stream = accept_before(&listener, deadline)?;
                let Some(request) = read_request(&mut stream)? else {
                    continue;
                };
                let Some(path) = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let route = routes.iter().find(|route| path.contains(&route.path));
                let (status, body) = route
                    .map(|route| (route.status, route.body.as_str()))
                    .unwrap_or((404, "{}"));
                write_response(&mut stream, status, body)?;
                captured.lock().unwrap().push(path);
            }
            (captured.lock().unwrap().len() == expected)
                .then_some(())
                .ok_or_else(|| "test HTTP server received fewer requests than expected".into())
        });
        TestServer {
            base_url: format!("http://{address}"),
            requests,
            worker: Some(worker),
        }
    }

    fn usage_sequence_server(responses: Vec<(u16, Value)>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for (status, body) in responses {
                let mut stream = accept_before(&listener, deadline)?;
                let Some(request) = read_request(&mut stream)? else {
                    return Err("test HTTP client disconnected before sending a request".into());
                };
                let body = body.to_string();
                write_response(&mut stream, status, &body)?;
                captured.lock().unwrap().push(request);
            }
            Ok(())
        });
        TestServer {
            base_url: format!("http://{address}"),
            requests,
            worker: Some(worker),
        }
    }

    fn route(path: &str, status: u16, body: Value) -> Route {
        Route {
            path: path.into(),
            status,
            body: body.to_string(),
        }
    }

    fn paid_body() -> Value {
        json!({
            "copilot_plan":"pro",
            "quota_reset_date":"2099-01-15T00:00:00Z",
            "quota_snapshots":{
                "premium_interactions":{
                    "entitlement":300,
                    "remaining":123,
                    "percent_remaining":41,
                    "overage_permitted":true,
                    "overage_count":2
                },
                "chat":{"unlimited":true,"entitlement":-1,"remaining":-1},
                "completions":{"entitlement":-1,"remaining":-1}
            }
        })
    }

    fn business_body() -> Value {
        json!({
            "copilot_plan":"business",
            "token_based_billing":true,
            "quota_snapshots":{
                "premium_interactions":{
                    "entitlement":0,
                    "remaining":0,
                    "unlimited":true,
                    "overage_permitted":true,
                    "overage_count":0
                }
            }
        })
    }

    fn org_summary(credits: f64, spend: f64) -> Value {
        json!({
            "usageItems":[{
                "product":"Copilot",
                "unitType":"ai-units",
                "grossQuantity":credits,
                "netAmount":spend
            }]
        })
    }

    fn single_response_provider(
        token: Option<&str>,
        usage_status: u16,
        body: Value,
    ) -> (CopilotProvider, TestServer) {
        let server = usage_sequence_server(vec![(usage_status, body)]);
        let provider = CopilotProvider::with_dependencies(
            CopilotAuthStore::for_test_token(token),
            CopilotClient::for_test(
                &server.base_url,
                "http://127.0.0.1:1",
                "http://127.0.0.1:1/",
                Duration::from_secs(1),
            ),
        );
        (provider, server)
    }

    fn routed_provider(
        token: Option<&str>,
        routes: Vec<Route>,
        expected: usize,
    ) -> (CopilotProvider, TestServer) {
        let server = routing_server(routes, expected);
        let provider = CopilotProvider::with_dependencies(
            CopilotAuthStore::for_test_token(token),
            CopilotClient::for_test(
                &format!("{}/copilot_internal/user", server.base_url),
                &format!("{}/user/orgs?per_page=100", server.base_url),
                &format!("{}/", server.base_url),
                Duration::from_secs(3),
            ),
        );
        (provider, server)
    }

    fn sequence_provider(
        tokens: &[&str],
        responses: Vec<(u16, Value)>,
    ) -> (CopilotProvider, TestServer) {
        let server = usage_sequence_server(responses);
        let provider = CopilotProvider::with_dependencies(
            CopilotAuthStore::for_test_tokens(tokens),
            CopilotClient::for_test(
                &server.base_url,
                "http://127.0.0.1:1",
                "http://127.0.0.1:1/",
                Duration::from_secs(1),
            ),
        );
        (provider, server)
    }

    #[test]
    fn organization_requests_are_capped_by_a_total_lookup_budget() {
        let timeout = org_request_timeout(Instant::now()).unwrap();
        assert!(timeout > Duration::ZERO);
        assert!(timeout <= ORG_REQUEST_TIMEOUT);

        let expired = Instant::now()
            .checked_sub(ORG_LOOKUP_BUDGET + Duration::from_millis(1))
            .unwrap();
        assert_eq!(org_request_timeout(expired), None);
    }

    #[test]
    fn successful_refresh_maps_plan_credits_and_exact_extra_usage() {
        let (provider, server) = single_response_provider(Some("secret-token"), 200, paid_body());
        let snapshot = provider.refresh().unwrap();
        server.finish();

        assert_eq!(snapshot.provider_id, "copilot");
        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        assert_eq!(snapshot.quotas[0].id, "premium");
        assert_eq!(snapshot.quotas[0].format, QuotaFormat::Count);
        assert_eq!(snapshot.quotas[0].unit.as_deref(), Some("credits"));
        assert_eq!(snapshot.value_metrics[0].id, "extra");
        assert_eq!(snapshot.value_metrics[0].values[0].number, 2.0);
        assert_eq!(
            snapshot.value_metrics[0].values[0].kind,
            MetricValueKind::Count
        );
        assert!(snapshot.warnings.is_empty());
    }

    #[test]
    fn refresh_uses_the_next_candidate_after_an_authentication_rejection() {
        let (provider, server) = sequence_provider(
            &["stale-token", "valid-token"],
            vec![(401, json!({})), (200, paid_body())],
        );

        let snapshot = provider.refresh().unwrap();
        let requests = server.finish();

        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("authorization: token stale-token"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("authorization: token valid-token"));
    }

    #[test]
    fn refresh_does_not_try_later_candidates_after_non_authentication_failures() {
        for (status, body, expected_kind) in [
            (429, json!({}), ProviderErrorKind::RateLimited),
            (200, Value::Null, ProviderErrorKind::InvalidResponse),
        ] {
            let (provider, server) =
                sequence_provider(&["first-token", "unused-token"], vec![(status, body)]);

            let error = provider.refresh().unwrap_err();
            let requests = server.finish();

            assert_eq!(error.kind(), expected_kind);
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn missing_invalid_rate_limited_and_malformed_states_are_typed() {
        let missing = CopilotProvider::with_dependencies(
            CopilotAuthStore::for_test_token(None),
            CopilotClient::for_test(
                "http://127.0.0.1:1",
                "http://127.0.0.1:1",
                "http://127.0.0.1:1/",
                Duration::from_millis(100),
            ),
        )
        .refresh()
        .unwrap_err();
        assert_eq!(missing.kind(), ProviderErrorKind::Authentication);
        assert!(missing.to_string().contains("gh auth login"));

        for status in [401, 403] {
            let (provider, server) =
                single_response_provider(Some("secret-token"), status, json!({}));
            let invalid = provider.refresh().unwrap_err();
            server.finish();
            assert_eq!(invalid.kind(), ProviderErrorKind::Authentication);
            assert!(!invalid.to_string().contains("secret-token"));
        }

        let (provider, server) = single_response_provider(Some("secret-token"), 429, json!({}));
        let rate_limited = provider.refresh().unwrap_err();
        server.finish();
        assert_eq!(rate_limited.kind(), ProviderErrorKind::RateLimited);

        let (provider, server) = single_response_provider(Some("secret-token"), 200, Value::Null);
        let malformed = provider.refresh().unwrap_err();
        server.finish();
        assert_eq!(malformed.kind(), ProviderErrorKind::InvalidResponse);

        let (provider, server) =
            single_response_provider(Some("secret-token"), 200, json!({"copilot_plan":"pro"}));
        let unavailable = provider.refresh().unwrap_err();
        server.finish();
        assert_eq!(unavailable.kind(), ProviderErrorKind::Permission);
    }

    #[test]
    fn org_managed_seat_uses_exact_organization_billing_metrics() {
        let (provider, server) = routed_provider(
            Some("secret-token"),
            vec![
                route("/copilot_internal/user", 200, business_body()),
                route("/user/orgs", 200, json!([{"login":"acme"}])),
                route(
                    "/orgs/acme/settings/billing/usage/summary",
                    200,
                    org_summary(298.698546, 1.25),
                ),
            ],
            3,
        );

        let snapshot = provider.refresh().unwrap();
        server.finish();

        assert_eq!(snapshot.plan.as_deref(), Some("Business"));
        assert!(snapshot.quotas.is_empty());
        assert_eq!(snapshot.value_metrics[0].id, "orgCredits");
        assert_eq!(snapshot.value_metrics[0].values[0].number, 298.698546);
        assert_eq!(snapshot.value_metrics[1].id, "orgSpend");
        assert_eq!(snapshot.value_metrics[1].values[0].number, 1.25);
        assert_eq!(provider.cached_org.lock().unwrap().as_deref(), Some("acme"));
    }

    #[test]
    fn optional_org_permission_and_malformed_responses_keep_the_plan_only_card() {
        for (status, body) in [(403, json!({})), (200, json!({"organization":"acme"}))] {
            let (provider, server) = routed_provider(
                Some("secret-token"),
                vec![
                    route("/copilot_internal/user", 200, business_body()),
                    route("/user/orgs", 200, json!([{"login":"acme"}])),
                    route("/orgs/acme/settings/billing/usage/summary", status, body),
                ],
                3,
            );
            let snapshot = provider.refresh().unwrap();
            server.finish();
            assert_eq!(snapshot.plan.as_deref(), Some("Business"));
            assert!(snapshot.quotas.is_empty());
            assert!(snapshot.value_metrics.is_empty());
        }
    }

    #[test]
    fn cached_org_skips_discovery_and_survives_transient_billing_failure() {
        let (provider, server) = routed_provider(
            Some("secret-token"),
            vec![
                route("/copilot_internal/user", 200, business_body()),
                route("/orgs/acme/settings/billing/usage/summary", 503, json!({})),
            ],
            2,
        );
        *provider.cached_org.lock().unwrap() = Some("acme".into());

        let snapshot = provider.refresh().unwrap();
        let requests = server.finish();

        assert!(snapshot.value_metrics.is_empty());
        assert_eq!(provider.cached_org.lock().unwrap().as_deref(), Some("acme"));
        let requests = requests.lock().unwrap();
        assert!(!requests.iter().any(|path| path.starts_with("/user/orgs")));
    }

    #[test]
    fn stale_cached_org_is_evicted_before_reprobing_other_orgs() {
        let (provider, server) = routed_provider(
            Some("secret-token"),
            vec![
                route("/copilot_internal/user", 200, business_body()),
                route("/orgs/old/settings/billing/usage/summary", 404, json!({})),
                route("/user/orgs", 200, json!([{"login":"new"}])),
                route(
                    "/orgs/new/settings/billing/usage/summary",
                    200,
                    org_summary(12.0, 0.0),
                ),
            ],
            4,
        );
        *provider.cached_org.lock().unwrap() = Some("old".into());

        let snapshot = provider.refresh().unwrap();
        server.finish();

        assert_eq!(snapshot.value_metrics[0].values[0].number, 12.0);
        assert_eq!(provider.cached_org.lock().unwrap().as_deref(), Some("new"));
    }

    #[test]
    fn one_transient_org_does_not_hide_a_later_readable_org() {
        let (provider, server) = routed_provider(
            Some("secret-token"),
            vec![
                route("/copilot_internal/user", 200, business_body()),
                route(
                    "/user/orgs",
                    200,
                    json!([{"login":"broken"},{"login":"acme"}]),
                ),
                route(
                    "/orgs/broken/settings/billing/usage/summary",
                    503,
                    json!({}),
                ),
                route(
                    "/orgs/acme/settings/billing/usage/summary",
                    200,
                    org_summary(42.0, 0.0),
                ),
            ],
            4,
        );

        let snapshot = provider.refresh().unwrap();
        server.finish();

        assert_eq!(snapshot.value_metrics[0].values[0].number, 42.0);
        assert_eq!(provider.cached_org.lock().unwrap().as_deref(), Some("acme"));
    }

    #[test]
    fn detection_and_refresh_use_the_same_auth_chain() {
        let (provider, server) = single_response_provider(Some("same-secret"), 200, paid_body());
        assert!(provider.has_local_credentials());
        assert!(provider.refresh().is_ok());
        server.finish();

        let missing = CopilotProvider::with_dependencies(
            CopilotAuthStore::for_test_token(None),
            CopilotClient::for_test(
                "http://127.0.0.1:1",
                "http://127.0.0.1:1",
                "http://127.0.0.1:1/",
                Duration::from_millis(100),
            ),
        );
        assert!(!missing.has_local_credentials());
        assert_eq!(
            missing.refresh().unwrap_err().kind(),
            ProviderErrorKind::Authentication
        );
    }

    #[test]
    fn definition_matches_the_product_layout_and_links() {
        let definition = definition();
        assert_eq!(definition.id, "copilot");
        assert!(!definition.fallback_enabled);
        assert_eq!(
            definition
                .metrics
                .iter()
                .map(|metric| metric.id.as_str())
                .collect::<Vec<_>>(),
            [
                "copilot.premium",
                "copilot.extra",
                "copilot.orgCredits",
                "copilot.orgSpend",
                "copilot.chat",
                "copilot.completions",
            ]
        );
        assert_eq!(
            definition.metrics[0].default_section,
            MetricSection::AlwaysVisible
        );
        assert!(definition.metrics[0].default_pinned);
        assert_eq!(
            definition.metrics[1].default_section,
            MetricSection::AlwaysVisible
        );
        assert!(!definition.metrics[1].default_pinned);
        assert!(definition.metrics[2..]
            .iter()
            .all(|metric| metric.default_section == MetricSection::OnDemand));
        assert_eq!(
            definition
                .links
                .iter()
                .map(|link| (link.label.as_str(), link.url.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Status", "https://www.githubstatus.com/"),
                ("Dashboard", "https://github.com/settings/billing"),
            ]
        );
    }
}
