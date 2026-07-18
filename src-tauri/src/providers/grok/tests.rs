use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use chrono::{TimeZone, Utc};
use tempfile::TempDir;

use super::{
    auth::GrokAuthStore, client::GrokClient, definition, local_usage::GrokLogUsageScanner,
    GrokProvider,
};
use crate::{
    models::{ProviderErrorKind, ProviderSnapshot, StatusTone, UsageHistory, UsagePeriod},
    pricing::PricingStore,
    providers::UsageProvider,
    storage::Storage,
};

const CREDITS: &str = include_str!("fixtures/credits.json");

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 18, 12, 0, 0).unwrap()
}

fn valid_auth() -> &'static str {
    r#"{
      "https://auth.x.ai::client-id": {
        "key": "old-token",
        "refresh_token": "refresh-token",
        "expires_at": "2027-01-01T00:00:00.000Z",
        "custom_field": "keep"
      }
    }"#
}

fn build_provider(
    directory: &TempDir,
    client: GrokClient,
    auth_json: &str,
    log_path: PathBuf,
) -> (GrokProvider, Arc<Storage>) {
    let auth_path = directory.path().join("auth.json");
    fs::write(&auth_path, auth_json).unwrap();
    let storage = Arc::new(Storage::open(&directory.path().join("openquota.db")).unwrap());
    let pricing = Arc::new(PricingStore::new(directory.path().join("pricing")).unwrap());
    (
        GrokProvider::with_dependencies(
            storage.clone(),
            pricing,
            GrokAuthStore::for_path(auth_path),
            client,
            GrokLogUsageScanner::for_path(log_path),
            now(),
        ),
        storage,
    )
}

#[test]
fn definition_matches_the_complete_default_layout() {
    let definition = definition();
    assert_eq!(definition.id, "grok");
    assert_eq!(
        definition.local_usage_source_note.as_deref(),
        Some("From your Grok logs (estimated)")
    );
    assert_eq!(
        definition
            .metrics
            .iter()
            .map(|metric| metric.id.as_str())
            .collect::<Vec<_>>(),
        [
            "grok.weekly",
            "grok.payAsYouGo",
            "grok.trend",
            "grok.today",
            "grok.yesterday",
            "grok.last30",
        ]
    );
    let weekly = &definition.metrics[0];
    assert_eq!(
        weekly.default_section,
        crate::models::MetricSection::AlwaysVisible
    );
    assert!(!weekly.default_pinned);
    let extra = &definition.metrics[1];
    assert_eq!(
        extra.default_section,
        crate::models::MetricSection::OnDemand
    );
    assert!(!extra.default_pinned);
}

#[test]
fn weekly_status_plan_and_local_history_form_one_snapshot() {
    let server = TestServer::new(2, |request| {
        if request.starts_with("GET /credits ") {
            (200, CREDITS.into())
        } else if request.starts_with("GET /settings ") {
            (
                200,
                r#"{"subscription_tier_display":"SuperGrok Heavy"}"#.into(),
            )
        } else {
            (404, "{}".into())
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let log_path = directory.path().join("unified.jsonl");
    fs::write(&log_path, include_str!("fixtures/usage.jsonl")).unwrap();
    let (provider, _) = build_provider(&directory, server.client(), valid_auth(), log_path);

    let snapshot = provider.refresh_inner().unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("SuperGrok Heavy"));
    assert_eq!(snapshot.quotas[0].id, "weekly");
    assert_eq!(snapshot.quotas[0].used_percent, 99.0);
    assert_eq!(snapshot.status_metrics[0].text, "Disabled");
    assert_eq!(snapshot.status_metrics[0].tone, StatusTone::Neutral);
    assert_eq!(snapshot.usage.today.unwrap().tokens, 2_000_000);
    assert!(snapshot.warnings.is_empty());
    server.finish();
}

#[test]
fn billing_auth_failure_refreshes_once_persists_and_retries() {
    let mut credits_calls = 0;
    let server = TestServer::new(4, move |request| {
        if request.starts_with("GET /credits ") {
            credits_calls += 1;
            if credits_calls == 1 {
                (401, "{}".into())
            } else {
                (200, CREDITS.into())
            }
        } else if request.starts_with("POST /token ") {
            (
                200,
                r#"{
                  "access_token":"new-token",
                  "refresh_token":"new-refresh",
                  "id_token":"new-id",
                  "expires_in":3600
                }"#
                .into(),
            )
        } else if request.starts_with("GET /settings ") {
            (200, r#"{"subscription_tier_display":"Heavy"}"#.into())
        } else {
            (404, "{}".into())
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let auth_path = directory.path().join("auth.json");
    let (provider, _) = build_provider(
        &directory,
        server.client(),
        valid_auth(),
        directory.path().join("missing-log.jsonl"),
    );

    let snapshot = provider.refresh_inner().unwrap();

    assert_eq!(snapshot.quotas[0].used_percent, 99.0);
    let requests = server.requests();
    let credit_auth = requests
        .iter()
        .filter(|request| request.starts_with("GET /credits "))
        .map(|request| header(request, "authorization").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        credit_auth,
        ["Bearer old-token".to_owned(), "Bearer new-token".to_owned()]
    );
    let refresh_request = requests
        .iter()
        .find(|request| request.starts_with("POST /token "))
        .unwrap();
    assert!(refresh_request.contains("client_id=client-id"));
    assert!(refresh_request.contains("refresh_token=refresh-token"));
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(auth_path).unwrap()).unwrap();
    let entry = &saved["https://auth.x.ai::client-id"];
    assert_eq!(entry["key"], "new-token");
    assert_eq!(entry["refresh_token"], "new-refresh");
    assert_eq!(entry["id_token"], "new-id");
    assert_eq!(entry["expires_at"], "2026-06-18T13:00:00.000Z");
    assert_eq!(entry["custom_field"], "keep");
    server.finish();
}

#[test]
fn expired_first_account_does_not_hide_a_usable_second_account() {
    let server = TestServer::new(2, |request| {
        if request.starts_with("GET /credits ") {
            (200, CREDITS.into())
        } else if request.starts_with("GET /settings ") {
            (200, "{}".into())
        } else {
            (404, "{}".into())
        }
    });
    let auth = r#"{
      "account-a": {
        "key": "expired",
        "expires_at": "2026-01-01T00:00:00.000Z"
      },
      "account-b": {
        "key": "working",
        "expires_at": "2027-01-01T00:00:00.000Z"
      }
    }"#;
    let directory = tempfile::tempdir().unwrap();
    let (provider, _) = build_provider(
        &directory,
        server.client(),
        auth,
        directory.path().join("missing-log.jsonl"),
    );

    let snapshot = provider.refresh_inner().unwrap();

    assert_eq!(snapshot.provider_id, "grok");
    let request = server
        .requests()
        .into_iter()
        .find(|request| request.starts_with("GET /credits "))
        .unwrap();
    assert_eq!(
        header(&request, "authorization").as_deref(),
        Some("Bearer working")
    );
    server.finish();
}

#[test]
fn monthly_accounts_keep_extra_usage_without_a_fake_weekly_meter() {
    let monthly = CREDITS.replace("USAGE_PERIOD_TYPE_WEEKLY", "USAGE_PERIOD_TYPE_MONTHLY");
    let server = TestServer::new(2, move |request| {
        if request.starts_with("GET /credits ") {
            (200, monthly.clone())
        } else if request.starts_with("GET /settings ") {
            (200, "{}".into())
        } else {
            (404, "{}".into())
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let (provider, _) = build_provider(
        &directory,
        server.client(),
        valid_auth(),
        directory.path().join("missing-log.jsonl"),
    );

    let snapshot = provider.refresh_inner().unwrap();

    assert!(snapshot.quotas.is_empty());
    assert_eq!(snapshot.status_metrics[0].text, "Disabled");
    server.finish();
}

#[test]
fn settings_is_optional_but_billing_failure_or_schema_drift_is_not() {
    let optional = TestServer::new(2, |request| {
        if request.starts_with("GET /credits ") {
            (200, CREDITS.into())
        } else {
            (503, "{}".into())
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let (provider, _) = build_provider(
        &directory,
        optional.client(),
        valid_auth(),
        directory.path().join("missing-log.jsonl"),
    );
    assert_eq!(provider.refresh_inner().unwrap().plan, None);
    optional.finish();

    for (status, body, expected_kind) in [
        (503, "{}", ProviderErrorKind::Network),
        (200, r#"{"config":{}}"#, ProviderErrorKind::InvalidResponse),
        (429, "{}", ProviderErrorKind::RateLimited),
    ] {
        let server = TestServer::new(1, move |_| (status, body.into()));
        let directory = tempfile::tempdir().unwrap();
        let (provider, _) = build_provider(
            &directory,
            server.client(),
            valid_auth(),
            directory.path().join("missing-log.jsonl"),
        );
        let error = UsageProvider::refresh(&provider).unwrap_err();
        assert_eq!(error.kind(), expected_kind);
        server.finish();
    }
}

#[test]
fn local_scan_failure_keeps_cached_history_and_live_limits() {
    let server = TestServer::new(2, |request| {
        if request.starts_with("GET /credits ") {
            (200, CREDITS.into())
        } else {
            (200, "{}".into())
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let bad_log_path = directory.path().join("unified.jsonl");
    fs::create_dir(&bad_log_path).unwrap();
    let (provider, storage) =
        build_provider(&directory, server.client(), valid_auth(), bad_log_path);
    let cached_usage = UsageHistory {
        today: Some(UsagePeriod {
            tokens: 42,
            estimated_cost_usd: Some(0.5),
            cost_estimated: true,
            estimate_complete: true,
            model_breakdown: None,
            unknown_models: Vec::new(),
        }),
        ..UsageHistory::default()
    };
    storage
        .save_snapshot(&ProviderSnapshot {
            provider_id: "grok".into(),
            plan: None,
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: cached_usage.clone(),
            warnings: Vec::new(),
            refreshed_at: now(),
        })
        .unwrap();

    let snapshot = provider.refresh_inner().unwrap();

    assert_eq!(snapshot.usage, cached_usage);
    assert_eq!(snapshot.quotas[0].used_percent, 99.0);
    assert_eq!(snapshot.warnings.len(), 1);
    assert!(snapshot.warnings[0].contains("Grok"));
    server.finish();
}

#[test]
fn credential_probe_and_missing_login_are_typed() {
    let server = TestServer::new(0, |_| (500, "{}".into()));
    let directory = tempfile::tempdir().unwrap();
    let (provider, _) = build_provider(
        &directory,
        server.client(),
        valid_auth(),
        directory.path().join("missing-log.jsonl"),
    );
    assert!(provider.has_local_credentials());
    server.finish();

    let server = TestServer::new(0, |_| (500, "{}".into()));
    let directory = tempfile::tempdir().unwrap();
    let (provider, _) = build_provider(
        &directory,
        server.client(),
        r#"{"account":{"refresh_token":"refresh-only"}}"#,
        directory.path().join("missing-log.jsonl"),
    );
    assert!(!provider.has_local_credentials());
    let error = UsageProvider::refresh(&provider).unwrap_err();
    assert_eq!(error.kind(), ProviderErrorKind::Authentication);
    server.finish();
}

struct TestServer {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: thread::JoinHandle<usize>,
    expected: usize,
}

impl TestServer {
    fn new(
        expected: usize,
        mut handler: impl FnMut(&str) -> (u16, String) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut handled = 0;
            while handled < expected && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let request = read_request(&mut stream);
                captured.lock().unwrap().push(request.clone());
                let (status, body) = handler(&request);
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                handled += 1;
            }
            handled
        });
        Self {
            base: format!("http://{address}"),
            requests,
            handle,
            expected,
        }
    }

    fn client(&self) -> GrokClient {
        GrokClient::for_test(
            &format!("{}/credits", self.base),
            &format!("{}/settings", self.base),
            &format!("{}/token", self.base),
            Duration::from_secs(3),
        )
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn finish(self) {
        assert_eq!(self.handle.join().unwrap(), self.expected);
    }
}

fn read_request(stream: &mut impl Read) -> String {
    let mut request = Vec::new();
    loop {
        let mut chunk = [0_u8; 2048];
        let Ok(count) = stream.read(&mut chunk) else {
            break;
        };
        if count == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
        let text = String::from_utf8_lossy(&request);
        let Some(header_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let content_length = text[..header_end]
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|value| value.parse::<usize>().ok())
            })
            .unwrap_or(0);
        if request.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

fn header(request: &str, expected_name: &str) -> Option<String> {
    request.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(expected_name)
            .then(|| value.trim().to_owned())
    })
}
