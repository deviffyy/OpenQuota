use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use serde_json::{json, Value};
use tempfile::tempdir;

use super::{
    auth::{DevinAuth, DevinAuthStore, DEFAULT_API_SERVER_URL},
    client::{DevinClient, CLOUD_COMPAT_VERSION},
    definition,
    mapper::{map_user_status_response, DAY_PERIOD_SECONDS, WEEK_PERIOD_SECONDS},
    select_failure, DevinError, DevinProvider,
};
use crate::{
    models::{MetricSection, MetricValueKind, ProviderErrorKind},
    providers::UsageProvider,
};

fn write_credentials(path: &Path, api_key: &str, api_server_url: Option<&str>) {
    let server = api_server_url
        .map(|url| format!("\napi_server_url = \"{url}\""))
        .unwrap_or_default();
    fs::write(path, format!("windsurf_api_key = \"{api_key}\"{server}\n")).unwrap();
}

fn write_state_db(path: &Path, value: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            ("windsurfAuthStatus", value.as_bytes()),
        )
        .unwrap();
}

fn auth(api_key: &str, api_server_url: &str) -> DevinAuth {
    DevinAuth {
        api_key: api_key.into(),
        api_server_url: Some(api_server_url.into()),
    }
}

fn provider(auth: DevinAuthStore, timeout: Duration) -> DevinProvider {
    DevinProvider::with_dependencies(
        auth,
        DevinClient::for_test(timeout),
        Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap(),
    )
}

#[test]
fn credentials_file_and_app_database_use_the_same_nonempty_token_filter() {
    let directory = tempdir().unwrap();
    let credentials = directory.path().join("credentials.toml");
    let state = directory.path().join("state.vscdb");
    write_credentials(
        &credentials,
        "  devin-session-token$cli  ",
        Some("https://server.codeium.test///"),
    );
    write_state_db(&state, r#"{"apiKey":"  devin-session-token$app  "}"#);
    let store = DevinAuthStore::with_paths(vec![credentials], vec![state]);

    let cli = store.load_credentials_file().unwrap();
    assert_eq!(cli.api_key, "devin-session-token$cli");
    assert_eq!(
        cli.api_server_url.as_deref(),
        Some("https://server.codeium.test")
    );
    let app = store.load_app_auth().unwrap();
    assert_eq!(app.api_key, "devin-session-token$app");
    assert_eq!(app.effective_api_server_url(), DEFAULT_API_SERVER_URL);
    assert!(store.has_local_credentials());
}

#[test]
fn malformed_candidates_are_isolated_and_usable_paths_keep_their_order() {
    let directory = tempdir().unwrap();
    let malformed_credentials = directory.path().join("bad.toml");
    let first_credentials = directory.path().join("first.toml");
    let second_credentials = directory.path().join("second.toml");
    fs::write(&malformed_credentials, "windsurf_api_key = \"unterminated").unwrap();
    write_credentials(&first_credentials, "first-token", None);
    write_credentials(&second_credentials, "second-token", None);

    let corrupt_db = directory.path().join("corrupt.vscdb");
    let drifted_db = directory.path().join("drifted.vscdb");
    let valid_db = directory.path().join("valid.vscdb");
    fs::write(&corrupt_db, "not sqlite").unwrap();
    Connection::open(&drifted_db)
        .unwrap()
        .execute("CREATE TABLE NewItemTable (payload TEXT)", [])
        .unwrap();
    write_state_db(&valid_db, r#"{"apiKey":"app-token"}"#);

    let store = DevinAuthStore::with_paths(
        vec![malformed_credentials, first_credentials, second_credentials],
        vec![corrupt_db, drifted_db, valid_db],
    );
    assert_eq!(
        store.load_credentials_file().unwrap().api_key,
        "first-token"
    );
    assert_eq!(
        store
            .load_credentials_files()
            .into_iter()
            .map(|auth| auth.api_key)
            .collect::<Vec<_>>(),
        ["first-token", "second-token"]
    );
    assert_eq!(store.load_app_auth().unwrap().api_key, "app-token");
}

#[test]
fn candidate_chain_keeps_file_then_app_order_and_deduplicates_equivalent_logins() {
    let directory = tempdir().unwrap();
    let first_credentials = directory.path().join("first.toml");
    let duplicate_credentials = directory.path().join("duplicate.toml");
    let duplicate_app = directory.path().join("duplicate.vscdb");
    let distinct_app = directory.path().join("distinct.vscdb");
    write_credentials(&first_credentials, "shared-token", None);
    write_credentials(
        &duplicate_credentials,
        "shared-token",
        Some(DEFAULT_API_SERVER_URL),
    );
    write_state_db(&duplicate_app, r#"{"apiKey":"shared-token"}"#);
    write_state_db(&distinct_app, r#"{"apiKey":"app-token"}"#);

    let candidates = DevinAuthStore::with_paths(
        vec![first_credentials, duplicate_credentials],
        vec![duplicate_app, distinct_app],
    )
    .load_candidates();

    assert_eq!(
        candidates
            .into_iter()
            .map(|auth| auth.api_key)
            .collect::<Vec<_>>(),
        ["shared-token", "app-token"]
    );
}

#[test]
fn a_locked_database_does_not_block_a_later_app_auth_candidate() {
    let directory = tempdir().unwrap();
    let locked_db = directory.path().join("locked.vscdb");
    let valid_db = directory.path().join("valid.vscdb");
    write_state_db(&locked_db, r#"{"apiKey":"locked-token"}"#);
    write_state_db(&valid_db, r#"{"apiKey":"fallback-token"}"#);
    let lock = Connection::open(&locked_db).unwrap();
    lock.execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE;")
        .unwrap();

    let store = DevinAuthStore::with_paths(Vec::new(), vec![locked_db, valid_db]);
    assert_eq!(store.load_app_auth().unwrap().api_key, "fallback-token");
}

#[test]
fn empty_malformed_or_wrongly_typed_app_auth_is_not_detected() {
    for payload in [
        "{}",
        r#"{"apiKey":""}"#,
        r#"{"apiKey":"   "}"#,
        r#"{"apiKey":42}"#,
        "not-json",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("state.vscdb");
        write_state_db(&path, payload);
        let store = DevinAuthStore::with_paths(Vec::new(), vec![path]);
        assert_eq!(store.load_app_auth(), None);
        assert!(!store.has_local_credentials());
    }
}

#[test]
fn mapper_preserves_resets_zeroes_and_exact_metric_units() {
    let body: Value = serde_json::from_str(include_str!("fixtures/user_status.json")).unwrap();
    let mapped = map_user_status_response(&body).unwrap();

    assert_eq!(mapped.plan.as_deref(), Some("Max"));
    assert_eq!(mapped.quotas.len(), 2);
    assert_eq!(mapped.quotas[0].id, "daily");
    assert_eq!(mapped.quotas[0].used_percent, 0.0);
    assert_eq!(mapped.quotas[0].period_seconds, DAY_PERIOD_SECONDS);
    assert_eq!(
        mapped.quotas[0].resets_at,
        Utc.timestamp_opt(1_774_080_000, 0).single()
    );
    assert_eq!(mapped.quotas[1].id, "weekly");
    assert_eq!(mapped.quotas[1].used_percent, 60.0);
    assert_eq!(mapped.quotas[1].period_seconds, WEEK_PERIOD_SECONDS);
    assert_eq!(
        mapped.quotas[1].resets_at,
        Utc.timestamp_opt(1_774_166_400, 0).single()
    );
    let balance = &mapped.value_metrics[0];
    assert_eq!(balance.id, "extraUsageBalance");
    assert_eq!(balance.values[0].kind, MetricValueKind::Dollars);
    assert!((balance.values[0].number - 964.22).abs() < 0.0001);
    assert!(!balance.values[0].estimated);

    let zero = map_user_status_response(&json!({
        "userStatus": {"planStatus": {"overageBalanceMicros": "0"}}
    }))
    .unwrap();
    assert_eq!(zero.plan.as_deref(), Some("Unknown"));
    assert_eq!(zero.value_metrics[0].values[0].number, 0.0);
}

#[test]
fn hidden_daily_fills_only_an_absent_weekly_meter_and_is_still_flipped() {
    let mapped = map_user_status_response(&json!({
        "userStatus": {"planStatus": {
            "planInfo": {"planName": " Teams ", "hideDailyQuota": true},
            "dailyQuotaRemainingPercent": 30,
            "weeklyQuotaResetAtUnix": 1774166400
        }}
    }))
    .unwrap();
    assert_eq!(mapped.plan.as_deref(), Some("Teams"));
    assert_eq!(mapped.quotas.len(), 1);
    assert_eq!(mapped.quotas[0].id, "weekly");
    assert_eq!(mapped.quotas[0].used_percent, 70.0);
    assert_eq!(mapped.quotas[0].period_seconds, WEEK_PERIOD_SECONDS);

    let explicit_weekly = map_user_status_response(&json!({
        "userStatus": {"planStatus": {
            "planInfo": {"hideDailyQuota": true},
            "dailyQuotaRemainingPercent": 30,
            "weeklyQuotaRemainingPercent": 0
        }}
    }))
    .unwrap();
    assert_eq!(explicit_weekly.quotas[0].used_percent, 100.0);

    for flag in [json!("true"), json!(1)] {
        let compatible = map_user_status_response(&json!({
            "userStatus": {"planStatus": {
                "planInfo": {"hideDailyQuota": flag},
                "dailyQuotaRemainingPercent": 30
            }}
        }))
        .unwrap();
        assert_eq!(compatible.quotas.len(), 1);
        assert_eq!(compatible.quotas[0].id, "weekly");
    }
}

#[test]
fn partial_fields_remain_usable_while_malformed_envelopes_fail_safely() {
    let daily_only = map_user_status_response(&json!({
        "userStatus": {"planStatus": {
            "dailyQuotaRemainingPercent": "-5",
            "dailyQuotaResetAtUnix": "not-a-time"
        }}
    }))
    .unwrap();
    assert_eq!(daily_only.quotas[0].used_percent, 100.0);
    assert_eq!(daily_only.quotas[0].resets_at, None);

    for body in [
        Value::Null,
        json!({}),
        json!({"userStatus":[]}),
        json!({"userStatus":{"planStatus":{"planInfo":{"planName":"Max"}}}}),
        json!({"userStatus":{"planStatus":{
            "dailyQuotaRemainingPercent":"nan",
            "overageBalanceMicros":"nan"
        }}}),
    ] {
        assert!(matches!(
            map_user_status_response(&body),
            Err(DevinError::InvalidResponse | DevinError::QuotaUnavailable)
        ));
    }
}

#[test]
fn connect_request_uses_the_expected_path_headers_and_metadata_shape() {
    let (base, request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let client = DevinClient::for_test(Duration::from_secs(1));
    let response = client
        .fetch_user_status(&auth("wire-secret", &base))
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);

    let request = request.recv_timeout(Duration::from_secs(1)).unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(headers
        .starts_with("POST /exa.seat_management_pb.SeatManagementService/GetUserStatus HTTP/1.1"));
    let lowercase_headers = headers.to_ascii_lowercase();
    assert!(lowercase_headers.contains("content-type: application/json"));
    assert!(lowercase_headers.contains("connect-protocol-version: 1"));
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["metadata"]["apiKey"], "wire-secret");
    assert_eq!(body["metadata"]["ideName"], "devin");
    assert_eq!(body["metadata"]["ideVersion"], CLOUD_COMPAT_VERSION);
    assert_eq!(body["metadata"]["extensionName"], "devin");
    assert_eq!(body["metadata"]["extensionVersion"], CLOUD_COMPAT_VERSION);
    assert_eq!(body["metadata"]["locale"], "en");
}

#[test]
fn credential_file_precedes_app_auth_and_app_auth_is_a_failure_fallback() {
    let (cli_base, cli_request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let (_, unused_app_request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let provider = provider(
        DevinAuthStore::with_paths(Vec::new(), Vec::new()),
        Duration::from_secs(1),
    );
    let snapshot = provider
        .refresh_candidates(
            Some(auth("cli-token", &cli_base)),
            Some(auth("app-token", "http://127.0.0.1:1")),
        )
        .unwrap();
    assert_eq!(snapshot.plan.as_deref(), Some("Max"));
    let request = cli_request.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(request.contains("cli-token"));
    assert!(unused_app_request
        .recv_timeout(Duration::from_millis(100))
        .is_err());

    let (expired_base, _) = capture_once(401, "{}", Duration::ZERO);
    let (app_base, app_request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let snapshot = provider
        .refresh_candidates(
            Some(auth("expired-token", &expired_base)),
            Some(auth("app-token", &app_base)),
        )
        .unwrap();
    assert_eq!(snapshot.quotas.len(), 2);
    assert!(app_request
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .contains("app-token"));
}

#[test]
fn authentication_failure_falls_through_every_ordered_candidate() {
    let (expired_base, expired_request) = capture_once(403, "{}", Duration::ZERO);
    let (valid_base, valid_request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let (unused_base, unused_request) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::ZERO,
    );
    let provider = provider(
        DevinAuthStore::with_paths(Vec::new(), Vec::new()),
        Duration::from_secs(1),
    );

    let snapshot = provider
        .refresh_auth_candidates(vec![
            auth("expired-file-token", &expired_base),
            auth("valid-file-token", &valid_base),
            auth("unused-app-token", &unused_base),
        ])
        .unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("Max"));
    assert!(expired_request
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .contains("expired-file-token"));
    assert!(valid_request
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .contains("valid-file-token"));
    assert!(unused_request
        .recv_timeout(Duration::from_millis(100))
        .is_err());
}

#[test]
fn mixed_candidate_failures_do_not_mask_unavailability_as_authentication() {
    assert!(matches!(
        select_failure(vec![
            DevinError::RequestFailed(429),
            DevinError::AuthenticationFailed,
        ]),
        DevinError::RequestFailed(429)
    ));
    assert!(matches!(
        select_failure(vec![
            DevinError::AuthenticationFailed,
            DevinError::ConnectionFailed,
        ]),
        DevinError::ConnectionFailed
    ));
    assert!(matches!(
        select_failure(vec![
            DevinError::AuthenticationFailed,
            DevinError::AuthenticationFailed,
        ]),
        DevinError::AuthenticationFailed
    ));
}

#[test]
fn auth_rate_transport_and_malformed_failures_keep_distinct_categories() {
    for status in [401, 403] {
        let (base, _) = capture_once(status, "{}", Duration::ZERO);
        let error = provider(
            DevinAuthStore::with_paths(Vec::new(), Vec::new()),
            Duration::from_secs(1),
        )
        .refresh_candidates(Some(auth("private-token", &base)), None)
        .unwrap_err();
        let error = crate::providers::ProviderError::from(error);
        assert_eq!(error.kind(), ProviderErrorKind::Authentication);
        assert!(!error.to_string().contains("private-token"));
    }

    let (base, _) = capture_once(429, "{}", Duration::ZERO);
    let rate = crate::providers::ProviderError::from(
        provider(
            DevinAuthStore::with_paths(Vec::new(), Vec::new()),
            Duration::from_secs(1),
        )
        .refresh_candidates(Some(auth("rate-secret", &base)), None)
        .unwrap_err(),
    );
    assert_eq!(rate.kind(), ProviderErrorKind::RateLimited);

    let network = crate::providers::ProviderError::from(
        provider(
            DevinAuthStore::with_paths(Vec::new(), Vec::new()),
            Duration::from_millis(100),
        )
        .refresh_candidates(Some(auth("network-secret", "http://127.0.0.1:1")), None)
        .unwrap_err(),
    );
    assert_eq!(network.kind(), ProviderErrorKind::Network);
    assert!(!network.to_string().contains("network-secret"));

    let (base, _) = capture_once(200, "not-json", Duration::ZERO);
    let malformed = crate::providers::ProviderError::from(
        provider(
            DevinAuthStore::with_paths(Vec::new(), Vec::new()),
            Duration::from_secs(1),
        )
        .refresh_candidates(Some(auth("decode-secret", &base)), None)
        .unwrap_err(),
    );
    assert_eq!(malformed.kind(), ProviderErrorKind::InvalidResponse);
}

#[test]
fn timeout_is_bounded_and_no_credentials_returns_a_login_hint() {
    let (base, _) = capture_once(
        200,
        include_str!("fixtures/user_status.json"),
        Duration::from_secs(1),
    );
    let timeout = provider(
        DevinAuthStore::with_paths(Vec::new(), Vec::new()),
        Duration::from_millis(100),
    )
    .refresh_candidates(Some(auth("timeout-secret", &base)), None)
    .unwrap_err();
    assert_eq!(timeout, DevinError::ConnectionFailed);

    let no_auth = provider(
        DevinAuthStore::with_paths(Vec::new(), Vec::new()),
        Duration::from_secs(1),
    )
    .refresh()
    .unwrap_err();
    assert_eq!(no_auth.kind(), ProviderErrorKind::Authentication);
    assert!(no_auth.to_string().contains("devin auth login"));
}

#[test]
fn definition_matches_the_provider_neutral_layout_contract() {
    let definition = definition();
    assert_eq!(definition.id, "devin");
    assert_eq!(definition.display_name, "Devin");
    assert_eq!(
        definition
            .links
            .iter()
            .map(|link| link.label.as_str())
            .collect::<Vec<_>>(),
        ["Dashboard"]
    );
    assert_eq!(
        definition
            .metrics
            .iter()
            .map(|metric| metric.id.as_str())
            .collect::<Vec<_>>(),
        ["devin.daily", "devin.weekly", "devin.extra"]
    );
    assert_eq!(
        definition.metrics[0].default_section,
        MetricSection::AlwaysVisible
    );
    assert_eq!(
        definition.metrics[1].default_section,
        MetricSection::AlwaysVisible
    );
    assert_eq!(
        definition.metrics[2].default_section,
        MetricSection::OnDemand
    );
    assert!(!definition
        .metrics
        .iter()
        .any(|metric| metric.default_pinned));
}

fn capture_once(status: u16, body: &str, delay: Duration) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_owned();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut request = Vec::new();
        loop {
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).unwrap_or_default();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if request_is_complete(&request) {
                break;
            }
        }
        let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
        thread::sleep(delay);
        let reason = match status {
            200 => "OK",
            401 => "Unauthorized",
            403 => "Forbidden",
            429 => "Too Many Requests",
            _ => "Test Response",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}"), receiver)
}

fn request_is_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
        return false;
    };
    let header_end = header_end + 4;
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    content_length.is_some_and(|length| request.len() >= header_end + length)
}
