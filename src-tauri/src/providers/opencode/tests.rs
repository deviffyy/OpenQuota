use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection};
use tempfile::tempdir;

use crate::{
    models::{MetricSection, ProviderErrorKind},
    pricing::{ModelPricing, ModelRates, PricingCatalog, PricingStore, PricingSupplement},
    providers::UsageProvider,
};

use super::{
    database::has_hosted_usage, definition, paths::OpenCodePaths, scanner::OpenCodeUsageScanner,
    OpenCodeError, OpenCodeProvider,
};

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap()
}

fn timestamp() -> i64 {
    Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0)
        .unwrap()
        .timestamp_millis()
}

fn pricing() -> ModelPricing {
    ModelPricing::new(
        PricingSupplement::default(),
        PricingCatalog {
            entries: HashMap::from([("priced-model".into(), ModelRates::new(1.0, 2.0))]),
            retrieved_at: None,
        },
        PricingCatalog::default(),
    )
}

fn pricing_store(directory: &Path) -> Arc<PricingStore> {
    Arc::new(PricingStore::new(directory.join("pricing")).unwrap())
}

fn create_database(path: &Path, with_parts: bool) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute("CREATE TABLE session (id TEXT PRIMARY KEY)", [])
        .unwrap();
    connection
        .execute(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
    if with_parts {
        connection
            .execute(
                "CREATE TABLE part (
                    id TEXT PRIMARY KEY,
                    message_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    data TEXT NOT NULL
                )",
                [],
            )
            .unwrap();
    }
    connection
}

fn insert_message(
    connection: &Connection,
    session_id: &str,
    message_id: &str,
    time_created: i64,
    data: &str,
) {
    connection
        .execute(
            "INSERT OR IGNORE INTO session (id) VALUES (?1)",
            [session_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO message (id, session_id, time_created, data)
             VALUES (?1, ?2, ?3, ?4)",
            params![message_id, session_id, time_created, data],
        )
        .unwrap();
}

fn exact_message(provider: &str, model: &str, cost: f64, input: u64, output: u64) -> String {
    serde_json::json!({
        "role": "assistant",
        "providerID": provider,
        "modelID": model,
        "cost": cost,
        "tokens": {
            "total": input + output,
            "input": input,
            "output": output,
            "reasoning": 0,
            "cache": {"read": 0, "write": 0}
        }
    })
    .to_string()
}

fn scan(paths: Vec<PathBuf>) -> super::scanner::OpenCodeUsageScan {
    OpenCodeUsageScanner::for_paths(paths.clone())
        .scan_paths(paths, now(), false, &pricing())
        .unwrap()
        .unwrap()
}

#[test]
fn definition_exposes_the_complete_metric_contract() {
    let definition = definition();
    assert_eq!(definition.id, "opencode");
    assert_eq!(definition.display_name, "OpenCode");
    assert_eq!(
        definition
            .metrics
            .iter()
            .map(|metric| metric.id.as_str())
            .collect::<Vec<_>>(),
        [
            "opencode.session",
            "opencode.weekly",
            "opencode.monthly",
            "opencode.trend",
            "opencode.today",
            "opencode.yesterday",
            "opencode.last30",
        ]
    );
    assert!(definition.metrics[..4]
        .iter()
        .all(|metric| metric.default_section == MetricSection::AlwaysVisible));
    assert!(definition.metrics[4..]
        .iter()
        .all(|metric| metric.default_section == MetricSection::OnDemand));
    assert!(definition
        .metrics
        .iter()
        .all(|metric| !metric.default_pinned));
}

#[test]
fn multiple_databases_are_merged_and_duplicate_messages_count_once() {
    let directory = tempdir().unwrap();
    let first_path = directory.path().join("opencode.db");
    let second_path = directory.path().join("opencode-next.db");
    let first = create_database(&first_path, false);
    insert_message(
        &first,
        "session-1",
        "message-shared",
        timestamp(),
        &exact_message("opencode-go", "priced-model", 1.0, 100, 0),
    );
    drop(first);

    let second = create_database(&second_path, false);
    insert_message(
        &second,
        "session-1",
        "message-shared",
        timestamp(),
        &exact_message("opencode-go", "priced-model", 2.0, 200, 0),
    );
    insert_message(
        &second,
        "session-2",
        "message-unique",
        timestamp() + 1_000,
        &exact_message("opencode", "priced-model", 3.0, 300, 0),
    );
    drop(second);

    let result = scan(vec![first_path, second_path]);
    let today = result.usage.today.unwrap();
    assert_eq!(today.tokens, 500);
    assert_eq!(today.estimated_cost_usd, Some(5.0));
    assert!(!today.cost_estimated);
    assert_eq!(result.go_windows.unwrap().session_spend, 2.0);
}

#[test]
fn costless_old_go_rows_do_not_shift_the_monthly_anchor() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, false);
    insert_message(
        &connection,
        "old-session",
        "old-message",
        Utc.with_ymd_and_hms(2026, 1, 31, 9, 0, 0)
            .unwrap()
            .timestamp_millis(),
        r#"{
            "role":"assistant",
            "providerID":"opencode-go",
            "modelID":"priced-model",
            "tokens":{"total":100,"input":100,"output":0}
        }"#,
    );
    insert_message(
        &connection,
        "current-session",
        "current-message",
        timestamp(),
        &exact_message("opencode-go", "priced-model", 2.0, 200, 0),
    );
    drop(connection);

    let windows = scan(vec![path]).go_windows.unwrap();
    assert_eq!(
        windows.monthly_resets_at,
        Utc.with_ymd_and_hms(2026, 8, 18, 10, 0, 0).unwrap()
    );
}

#[test]
fn message_totals_win_over_step_parts_without_double_counting() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, true);
    insert_message(
        &connection,
        "session-1",
        "message-1",
        timestamp(),
        &exact_message("opencode-go", "priced-model", 3.0, 100, 50),
    );
    connection
        .execute(
            "INSERT INTO part (id, message_id, session_id, time_created, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "part-1",
                "message-1",
                "session-1",
                timestamp(),
                serde_json::json!({
                    "type": "step-finish",
                    "cost": 3.0,
                    "tokens": {
                        "total": 150,
                        "input": 100,
                        "output": 50,
                        "reasoning": 0,
                        "cache": {"read": 0, "write": 0}
                    }
                })
                .to_string()
            ],
        )
        .unwrap();
    drop(connection);

    let today = scan(vec![path]).usage.today.unwrap();
    assert_eq!(today.tokens, 150);
    assert_eq!(today.estimated_cost_usd, Some(3.0));
}

#[test]
fn step_parts_fill_missing_message_totals_exactly() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, true);
    insert_message(
        &connection,
        "session-1",
        "message-1",
        timestamp(),
        r#"{"role":"assistant","providerID":"opencode","modelID":"priced-model"}"#,
    );
    for (id, cost, input) in [("part-1", 1.25, 100_u64), ("part-2", 2.75, 200)] {
        connection
            .execute(
                "INSERT INTO part (id, message_id, session_id, time_created, data)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    "message-1",
                    "session-1",
                    timestamp(),
                    serde_json::json!({
                        "type": "step-finish",
                        "cost": cost,
                        "tokens": {
                            "input": input,
                            "output": 0,
                            "reasoning": 0,
                            "cache": {"read": 0, "write": 0}
                        }
                    })
                    .to_string()
                ],
            )
            .unwrap();
    }
    drop(connection);

    let today = scan(vec![path]).usage.today.unwrap();
    assert_eq!(today.tokens, 300);
    assert_eq!(today.estimated_cost_usd, Some(4.0));
    assert!(!today.cost_estimated);
}

#[test]
fn epoch_second_rows_and_invalid_message_cost_fall_back_to_valid_parts() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, true);
    let epoch_seconds = timestamp().div_euclid(1_000);
    insert_message(
        &connection,
        "session-1",
        "message-1",
        epoch_seconds,
        r#"{
            "role":"assistant",
            "providerID":"opencode-go",
            "modelID":"priced-model",
            "cost":null
        }"#,
    );
    connection
        .execute(
            "INSERT INTO part (id, message_id, session_id, time_created, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "part-1",
                "message-1",
                "session-1",
                epoch_seconds,
                serde_json::json!({
                    "type": "step-finish",
                    "cost": 1.5,
                    "tokens": {
                        "input": 100,
                        "output": 25,
                        "reasoning": 0,
                        "cache": {"read": 0, "write": 0}
                    }
                })
                .to_string()
            ],
        )
        .unwrap();
    drop(connection);

    assert!(has_hosted_usage(&path).unwrap());
    let result = scan(vec![path]);
    let today = result.usage.today.unwrap();
    assert_eq!(today.tokens, 125);
    assert_eq!(today.estimated_cost_usd, Some(1.5));
    assert!(!today.cost_estimated);
    assert_eq!(
        result.go_windows.unwrap().monthly_resets_at,
        Utc.with_ymd_and_hms(2026, 8, 18, 10, 0, 0).unwrap()
    );
}

#[test]
fn missing_stored_cost_uses_pricing_and_marks_the_period_estimated() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, false);
    insert_message(
        &connection,
        "session-1",
        "message-1",
        timestamp(),
        r#"{
            "role":"assistant",
            "providerID":"opencode",
            "modelID":"priced-model",
            "tokens":{
                "total":1000000,
                "input":1000000,
                "output":0,
                "reasoning":0,
                "cache":{"read":0,"write":0}
            }
        }"#,
    );
    drop(connection);

    let today = scan(vec![path]).usage.today.unwrap();
    assert_eq!(today.estimated_cost_usd, Some(1.0));
    assert!(today.cost_estimated);
}

#[test]
fn estimated_go_spend_does_not_invent_subscription_caps() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = create_database(&path, false);
    insert_message(
        &connection,
        "session-1",
        "message-1",
        timestamp(),
        r#"{
            "role":"assistant",
            "providerID":"opencode-go",
            "modelID":"priced-model",
            "tokens":{
                "total":1000000,
                "input":1000000,
                "output":0
            }
        }"#,
    );
    drop(connection);

    let result = scan(vec![path]);
    let today = result.usage.today.unwrap();
    assert_eq!(today.estimated_cost_usd, Some(1.0));
    assert!(today.cost_estimated);
    assert!(result.go_windows.is_none());
}

#[test]
fn exact_zero_cost_is_usage_but_unknown_cost_stays_visible_as_incomplete() {
    let directory = tempdir().unwrap();
    let exact_path = directory.path().join("opencode.db");
    let exact = create_database(&exact_path, false);
    insert_message(
        &exact,
        "session-1",
        "message-zero",
        timestamp(),
        &exact_message("opencode", "priced-model", 0.0, 100, 0),
    );
    drop(exact);
    let exact_usage = scan(vec![exact_path]).usage;
    assert_eq!(
        exact_usage.today.as_ref().unwrap().estimated_cost_usd,
        Some(0.0)
    );
    assert!(!exact_usage.today.unwrap().cost_estimated);

    let unknown_path = directory.path().join("opencode-next.db");
    let unknown = create_database(&unknown_path, false);
    insert_message(
        &unknown,
        "session-2",
        "message-unknown",
        timestamp(),
        r#"{
            "role":"assistant",
            "providerID":"opencode",
            "modelID":"unknown-model",
            "tokens":{"input":50,"output":10,"cache":{"read":0,"write":0}}
        }"#,
    );
    drop(unknown);
    let unknown_usage = scan(vec![unknown_path]).usage;
    assert!(unknown_usage.today.is_none());
    assert_eq!(unknown_usage.unknown_models, ["unknown-model"]);
}

#[test]
fn data_only_message_schema_is_supported_and_orphans_are_ignored_when_sessions_exist() {
    let directory = tempdir().unwrap();
    let variant_path = directory.path().join("opencode-variant.db");
    let variant = Connection::open(&variant_path).unwrap();
    variant
        .execute(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT)",
            [],
        )
        .unwrap();
    variant
        .execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            params![
                "message-1",
                "session-1",
                serde_json::json!({
                    "role": "assistant",
                    "provider_id": "opencode",
                    "model_id": "priced-model",
                    "time_created": timestamp(),
                    "costUSD": 1.0,
                    "tokens": {"input": 10, "output": 5}
                })
                .to_string()
            ],
        )
        .unwrap();
    drop(variant);
    assert_eq!(scan(vec![variant_path]).usage.today.unwrap().tokens, 15);

    let orphan_path = directory.path().join("opencode-orphan.db");
    let orphan = create_database(&orphan_path, false);
    orphan
        .execute(
            "INSERT INTO message (id, session_id, time_created, data)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                "orphan",
                "missing-session",
                timestamp(),
                exact_message("opencode", "priced-model", 9.0, 900, 0)
            ],
        )
        .unwrap();
    drop(orphan);
    assert!(scan(vec![orphan_path]).usage.daily.is_empty());
}

#[test]
fn corrupt_missing_and_schema_drift_databases_do_not_blank_a_usable_source() {
    let directory = tempdir().unwrap();
    let good_path = directory.path().join("opencode.db");
    let good = create_database(&good_path, false);
    insert_message(
        &good,
        "session",
        "message",
        timestamp(),
        &exact_message("opencode", "priced-model", 1.0, 100, 0),
    );
    drop(good);

    let corrupt_path = directory.path().join("opencode-corrupt.db");
    fs::write(&corrupt_path, b"not a database").unwrap();
    let drift_path = directory.path().join("opencode-drift.db");
    let drift = Connection::open(&drift_path).unwrap();
    drift
        .execute("CREATE TABLE message (unexpected TEXT)", [])
        .unwrap();
    drop(drift);
    let missing_path = directory.path().join("opencode-missing.db");

    let result = scan(vec![corrupt_path, drift_path, missing_path, good_path]);
    assert_eq!(result.usage.today.unwrap().estimated_cost_usd, Some(1.0));
    assert_eq!(result.warnings.len(), 1);
    assert!(!result.warnings[0].contains(directory.path().to_string_lossy().as_ref()));
}

#[test]
fn all_present_unusable_databases_return_a_safe_typed_error() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    fs::write(&path, b"private-content-not-a-database").unwrap();
    let error = OpenCodeUsageScanner::for_paths(vec![path.clone()])
        .scan_paths(vec![path.clone()], now(), false, &pricing())
        .unwrap_err();
    assert_eq!(error, OpenCodeError::DatabaseUnreadable);
    assert!(!error.to_string().contains(path.to_string_lossy().as_ref()));
    assert!(!error.to_string().contains("private-content"));
}

#[test]
fn local_detection_requires_a_key_or_a_readable_hosted_usage_row() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path();
    let paths = OpenCodePaths::for_data_directory(data_directory.to_path_buf());
    let provider =
        OpenCodeProvider::with_dependencies(paths.clone(), pricing_store(data_directory), now());
    assert!(!provider.has_local_credentials());

    let empty = create_database(&data_directory.join("opencode.db"), false);
    drop(empty);
    assert!(!provider.has_local_credentials());

    let connection = Connection::open(data_directory.join("opencode.db")).unwrap();
    insert_message(
        &connection,
        "session",
        "message-without-cost",
        timestamp(),
        r#"{
            "role":"assistant",
            "providerID":"opencode",
            "modelID":"priced-model",
            "tokens":{"total":100,"input":100,"output":0}
        }"#,
    );
    assert!(!provider.has_local_credentials());
    insert_message(
        &connection,
        "session",
        "message",
        timestamp(),
        &exact_message("opencode", "priced-model", 0.0, 0, 0),
    );
    drop(connection);
    assert!(provider.has_local_credentials());
}

#[test]
fn go_key_without_database_shows_zero_estimated_caps() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("auth.json"),
        r#"{"opencode-go":{"type":"api","key":"secret-key"}}"#,
    )
    .unwrap();
    let provider = OpenCodeProvider::with_dependencies(
        OpenCodePaths::for_data_directory(directory.path().to_path_buf()),
        pricing_store(directory.path()),
        now(),
    );
    let snapshot = provider.refresh().unwrap();
    assert_eq!(snapshot.plan.as_deref(), Some("Go"));
    assert_eq!(snapshot.quotas.len(), 3);
    assert!(snapshot.quotas.iter().all(|quota| {
        quota.used_value == Some(0.0)
            && quota.estimated
            && quota.source_note.is_some()
            && quota.unit.as_deref() == Some("usd")
    }));
}

#[test]
fn malformed_auth_does_not_blank_valid_database_usage() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("auth.json"), "secret invalid json").unwrap();
    let connection = create_database(&directory.path().join("opencode.db"), false);
    insert_message(
        &connection,
        "session",
        "message",
        timestamp(),
        &exact_message("opencode", "priced-model", 1.0, 100, 0),
    );
    drop(connection);
    let provider = OpenCodeProvider::with_dependencies(
        OpenCodePaths::for_data_directory(directory.path().to_path_buf()),
        pricing_store(directory.path()),
        now(),
    );
    let snapshot = provider.refresh().unwrap();
    assert_eq!(snapshot.usage.today.unwrap().estimated_cost_usd, Some(1.0));
    assert_eq!(snapshot.warnings.len(), 1);
    assert!(!snapshot.warnings[0].contains("secret"));
}

#[test]
fn absent_and_unreadable_sources_map_to_distinct_safe_categories() {
    let directory = tempdir().unwrap();
    let absent = OpenCodeProvider::with_dependencies(
        OpenCodePaths::for_data_directory(directory.path().to_path_buf()),
        pricing_store(directory.path()),
        now(),
    )
    .refresh()
    .unwrap_err();
    assert_eq!(absent.kind(), ProviderErrorKind::Authentication);

    fs::write(directory.path().join("opencode.db"), b"corrupt").unwrap();
    let unreadable = OpenCodeProvider::with_dependencies(
        OpenCodePaths::for_data_directory(directory.path().to_path_buf()),
        pricing_store(directory.path()),
        now(),
    )
    .refresh()
    .unwrap_err();
    assert_eq!(unreadable.kind(), ProviderErrorKind::LocalData);
    assert!(!unreadable
        .to_string()
        .contains(directory.path().to_string_lossy().as_ref()));
}
