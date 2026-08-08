use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{
    models::UsageHistory,
    pricing::{ModelPricing, TokenBreakdown},
    storage::Storage,
};

use super::ClaudeError;
use crate::providers::{
    daily_usage::DailyUsageAccumulator,
    log_usage::{load_or_parse_log, parse_log_timestamp},
    pi_usage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeTokenEvent {
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>,
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    pub message_id: Option<String>,
    pub request_id: Option<String>,
    pub sidechain: bool,
    #[serde(default)]
    pub is_fast: bool,
    #[serde(default)]
    pub has_speed: bool,
    pub cost_usd: Option<f64>,
}

const LOG_CACHE_SCHEMA_VERSION: u8 = 3;

impl ClaudeTokenEvent {
    fn total_tokens(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }
}

pub fn scan_local_usage(
    storage: &Storage,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    provider_id: &str,
    configured_roots: &[PathBuf],
    include_standard_roots: bool,
    include_pi: bool,
) -> Result<UsageHistory, ClaudeError> {
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    let mut events = Vec::new();
    let paths = discover_files(configured_roots, include_standard_roots);
    let mut seen_paths = HashSet::with_capacity(paths.len());
    for path in paths {
        seen_paths.insert(path.clone());
        let Some(parsed) = load_or_parse_log(
            storage,
            provider_id,
            &path,
            LOG_CACHE_SCHEMA_VERSION,
            parse_jsonl,
        )
        .map_err(|_| ClaudeError::LocalUsage)?
        else {
            continue;
        };
        events.extend(
            parsed
                .into_iter()
                .filter(|event| event.timestamp.with_timezone(&Local).date_naive() >= since_date),
        );
    }
    storage
        .prune_log_events(provider_id, &seen_paths)
        .map_err(|_| ClaudeError::LocalUsage)?;
    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(deduplicate(events), now, pricing, &mut accumulator);
    let includes_pi = if include_pi {
        match pi_usage::scan_into(storage, now, pricing, provider_id, &mut accumulator) {
            Ok(includes_pi) => includes_pi,
            Err(_) => {
                crate::app_warn!(
                    "plugin:pi",
                    "pi usage history could not be folded into Claude"
                );
                false
            }
        }
    } else {
        false
    };
    let source_note = if includes_pi {
        "From your Claude usage history and pi (estimated)"
    } else {
        "From your Claude usage history (estimated)"
    };
    Ok(accumulator.build(now, source_note))
}

fn discover_files(configured_roots: &[PathBuf], include_standard_roots: bool) -> Vec<PathBuf> {
    let home = home_directory();
    let mut roots = if include_standard_roots {
        let config = env_text("CLAUDE_CONFIG_DIR");
        let xdg = env_text("XDG_CONFIG_HOME");
        claude_roots(config.as_deref(), xdg.as_deref(), &home)
    } else {
        Vec::new()
    };
    roots.extend(configured_roots.iter().cloned());
    roots.sort();
    roots.dedup();
    discover_files_in_roots(&roots)
}

fn discover_files_in_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = roots
        .iter()
        .flat_map(|root| {
            let projects = root.join("projects");
            let projects = fs::canonicalize(&projects).unwrap_or(projects);
            WalkDir::new(projects)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_type().is_file()
                        && entry.path().extension().and_then(|value| value.to_str())
                            == Some("jsonl")
                })
                .map(|entry| entry.into_path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn claude_roots(config: Option<&str>, xdg: Option<&str>, home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut add_if_valid = |root: PathBuf| {
        if root.join("projects").is_dir() && seen.insert(root.clone()) {
            roots.push(root);
        }
    };

    if let Some(config) = config.map(str::trim).filter(|value| !value.is_empty()) {
        for value in config
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let mut root = expand_home(value, home);
            if root.file_name().and_then(|name| name.to_str()) == Some("projects") {
                root.pop();
            }
            add_if_valid(root);
        }
    } else {
        let xdg = xdg
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| expand_home(value, home))
            .unwrap_or_else(|| home.join(".config"));
        add_if_valid(xdg.join("claude"));
        add_if_valid(home.join(".claude"));
    }

    for root in cowork_claude_roots(home) {
        add_if_valid(root);
    }
    roots
}

fn cowork_claude_roots(home: &Path) -> Vec<PathBuf> {
    let base = home.join("Library/Application Support/Claude/local-agent-mode-sessions");
    let mut roots = Vec::new();
    for group in child_directories(&base) {
        for subgroup in child_directories(&group) {
            let mut sessions = child_directories(&subgroup);
            let nested_agent_sessions = sessions
                .iter()
                .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("agent"))
                .flat_map(|agent| child_directories(agent))
                .collect::<Vec<_>>();
            sessions.extend(nested_agent_sessions);
            roots.extend(sessions.into_iter().map(|session| session.join(".claude")));
        }
    }
    roots.sort();
    roots
}

fn child_directories(path: &Path) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn env_text(name: &str) -> Option<String> {
    crate::provider_environment::value(name)
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home.join(rest);
    }
    PathBuf::from(value)
}

pub fn parse_jsonl(content: &str) -> Vec<ClaudeTokenEvent> {
    content
        .lines()
        .filter(|line| line.contains("\"usage\":{"))
        .filter(|line| !has_unsupported_null_field(line))
        .flat_map(|line| parse_entries(line).unwrap_or_default())
        .collect()
}

#[cfg(test)]
fn parse_line(line: &str) -> Option<ClaudeTokenEvent> {
    parse_entries(line)?.into_iter().next()
}

fn parse_entries(line: &str) -> Option<Vec<ClaudeTokenEvent>> {
    let object: Value = serde_json::from_str(line).ok()?;
    let timestamp = parse_log_timestamp(object.get("timestamp")?.as_str()?)?;
    let message = object.get("message")?;
    if object
        .get("version")
        .and_then(Value::as_str)
        .is_some_and(|version| !is_semver_prefix(version))
        || [
            object.get("sessionId"),
            object.get("requestId"),
            message.get("id"),
            message.get("model"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(str::is_empty)
    {
        return None;
    }
    let usage = message.get("usage")?;
    let message_id = message.get("id").and_then(Value::as_str).map(str::to_owned);
    let request_id = object
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sidechain = object
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let parent = usage_event(
        timestamp,
        message
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "<synthetic>")
            .map(str::to_owned),
        usage,
        message_id.clone(),
        request_id.clone(),
        sidechain,
        object.get("costUSD").and_then(number),
    )?;
    let mut events = vec![parent];
    let mut advisor_index = 0;
    for iteration in usage
        .get("iterations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if iteration.get("type").and_then(Value::as_str) != Some("advisor_message") {
            continue;
        }
        let Some(model) = iteration
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };
        let advisor_message_id = message_id
            .as_ref()
            .map(|message_id| format!("{message_id}:advisor:{advisor_index}"));
        let Some(advisor) = usage_event(
            timestamp,
            Some(model.to_owned()),
            iteration,
            advisor_message_id,
            request_id.clone(),
            sidechain,
            None,
        ) else {
            continue;
        };
        events.push(advisor);
        advisor_index += 1;
    }
    Some(events)
}

fn usage_event(
    timestamp: DateTime<Utc>,
    model: Option<String>,
    usage: &Value,
    message_id: Option<String>,
    request_id: Option<String>,
    sidechain: bool,
    cost_usd: Option<f64>,
) -> Option<ClaudeTokenEvent> {
    let input = integer(usage.get("input_tokens"))?;
    let output = integer(usage.get("output_tokens"))?;
    let speed = usage.get("speed").and_then(Value::as_str);
    if speed.is_some_and(|speed| !matches!(speed, "fast" | "standard")) {
        return None;
    }
    let (cache_write_5m, cache_write_1h) = usage
        .get("cache_creation")
        .map(|cache| {
            (
                integer(cache.get("ephemeral_5m_input_tokens")).unwrap_or_default(),
                integer(cache.get("ephemeral_1h_input_tokens")).unwrap_or_default(),
            )
        })
        .unwrap_or_else(|| {
            (
                integer(usage.get("cache_creation_input_tokens")).unwrap_or_default(),
                0,
            )
        });
    Some(ClaudeTokenEvent {
        timestamp,
        model,
        input,
        cache_write_5m,
        cache_write_1h,
        cache_read: integer(usage.get("cache_read_input_tokens")).unwrap_or_default(),
        output,
        message_id,
        request_id,
        sidechain,
        is_fast: speed == Some("fast"),
        has_speed: speed.is_some(),
        cost_usd,
    })
}

fn deduplicate(events: Vec<ClaudeTokenEvent>) -> Vec<ClaudeTokenEvent> {
    let mut output = Vec::new();
    let mut exact = HashMap::<(String, Option<String>), usize>::new();
    let mut by_message = HashMap::<String, Vec<usize>>::new();
    for event in events {
        let Some(message_id) = event.message_id.clone() else {
            output.push(event);
            continue;
        };
        let key = (message_id.clone(), event.request_id.clone());
        let collision = exact.get(&key).copied().or_else(|| {
            by_message.get(&message_id)?.iter().copied().find(|index| {
                event.sidechain
                    || output
                        .get(*index)
                        .is_some_and(|existing| existing.sidechain)
            })
        });
        if let Some(index) = collision {
            let existing = &output[index];
            let replace = (existing.sidechain && !event.sidechain)
                || (existing.sidechain == event.sidechain
                    && (event.total_tokens() > existing.total_tokens()
                        || (event.total_tokens() == existing.total_tokens()
                            && event.has_speed
                            && !existing.has_speed)));
            if replace {
                if let Some(old) = output.get(index) {
                    if let Some(old_message_id) = &old.message_id {
                        exact.remove(&(old_message_id.clone(), old.request_id.clone()));
                    }
                }
                output[index] = event;
                exact.insert(key, index);
            }
            continue;
        }
        let index = output.len();
        output.push(event);
        exact.insert(key, index);
        by_message.entry(message_id).or_default().push(index);
    }
    output
}

#[cfg(test)]
fn aggregate(
    events: Vec<ClaudeTokenEvent>,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
) -> UsageHistory {
    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(events, now, pricing, &mut accumulator);
    accumulator.build(now, "From your Claude usage history (estimated)")
}

fn aggregate_into(
    events: Vec<ClaudeTokenEvent>,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    accumulator: &mut DailyUsageAccumulator,
) {
    let since = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    for event in events {
        let date = event.timestamp.with_timezone(&Local).date_naive();
        if date < since {
            continue;
        }
        let tokens = TokenBreakdown {
            input: event.input,
            cache_write_5m: event.cache_write_5m,
            cache_write_1h: event.cache_write_1h,
            cache_read: event.cache_read,
            output: event.output,
            is_fast: event.is_fast,
        };
        let model_name = event
            .model
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let cost = event
            .cost_usd
            .or_else(|| pricing.estimated_cost_dollars(model_name?, tokens, true));
        if let Some(cost) = cost {
            accumulator.add(
                date,
                tokens.total_tokens(),
                cost,
                model_name.unwrap_or("Unattributed"),
            );
        } else if tokens.total_tokens() > 0 {
            if let Some(model) = model_name {
                accumulator.add_unknown_model(date, model);
            }
        }
    }
}

fn integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64()
}

fn is_semver_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    for component in 0..3 {
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
        if component < 2 {
            if bytes.get(index) != Some(&b'.') {
                return false;
            }
            index += 1;
        }
    }
    true
}

fn has_unsupported_null_field(line: &str) -> bool {
    const FIELDS: [&str; 11] = [
        "id",
        "cwd",
        "model",
        "speed",
        "costUSD",
        "version",
        "sessionId",
        "requestId",
        "isApiErrorMessage",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ];
    FIELDS
        .iter()
        .any(|field| line.contains(&format!("\"{field}\":null")))
}

#[cfg(test)]
mod tests {
    use std::{fs, io, path::Path};

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::{
        aggregate, claude_roots, deduplicate, discover_files, discover_files_in_roots,
        has_unsupported_null_field, is_semver_prefix, parse_jsonl, parse_line, ClaudeTokenEvent,
    };
    use crate::pricing::{test_bundled_pricing, TokenBreakdown};

    #[test]
    fn provider_fixture_parses_and_deduplicates_claude_usage_lines() {
        let events = parse_jsonl(include_str!("fixtures/usage.jsonl"));
        assert_eq!(events.len(), 2);
        let events = deduplicate(events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].total_tokens(), 170);
        let history = aggregate(events, chrono::Utc::now(), &test_bundled_pricing());
        assert!(history.last_30_days.unwrap().estimated_cost_usd.is_some());
    }

    #[test]
    fn parses_modern_cache_split_and_speed_fields() {
        let line = r#"{"timestamp":"2026-02-20T12:00:00.000Z","sessionId":"s","requestId":"req_1","version":"1.0.24","isSidechain":true,"costUSD":0.5,"message":{"id":"msg_1","model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":30,"cache_creation":{"ephemeral_5m_input_tokens":20,"ephemeral_1h_input_tokens":10},"speed":"fast"}}}"#;
        let event = parse_line(line).unwrap();
        assert_eq!(event.input, 100);
        assert_eq!(event.cache_write_5m, 20);
        assert_eq!(event.cache_write_1h, 10);
        assert_eq!(event.cache_read, 30);
        assert_eq!(event.output, 50);
        assert_eq!(event.message_id.as_deref(), Some("msg_1"));
        assert_eq!(event.request_id.as_deref(), Some("req_1"));
        assert!(event.sidechain);
        assert!(event.is_fast);
        assert!(event.has_speed);
        assert_eq!(event.cost_usd, Some(0.5));
    }

    #[test]
    fn expands_only_advisor_iterations_without_recounting_main_usage() {
        let line = r#"{"timestamp":"2026-02-20T12:00:00.000Z","requestId":"req_1","costUSD":1.23,"message":{"id":"msg_1","model":"main-model","usage":{"input_tokens":2,"output_tokens":491,"cache_creation_input_tokens":7853,"cache_read_input_tokens":226584,"iterations":[{"type":"message","input_tokens":1,"output_tokens":200},{"type":"advisor_message","model":"claude-opus-4-6","input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4},{"type":"message","input_tokens":1,"output_tokens":291}]}}}"#;

        let events = parse_jsonl(line);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].model.as_deref(), Some("main-model"));
        assert_eq!(events[0].input, 2);
        assert_eq!(events[0].cache_write_5m, 7853);
        assert_eq!(events[0].cache_read, 226584);
        assert_eq!(events[0].output, 491);
        assert_eq!(events[0].cost_usd, Some(1.23));
        assert_eq!(events[1].model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(events[1].message_id.as_deref(), Some("msg_1:advisor:0"));
        assert_eq!(events[1].request_id.as_deref(), Some("req_1"));
        assert_eq!(events[1].input, 10);
        assert_eq!(events[1].cache_write_5m, 3);
        assert_eq!(events[1].cache_read, 4);
        assert_eq!(events[1].output, 2);
        assert_eq!(events[1].cost_usd, None);

        let duplicated = format!("{line}\n{line}");
        assert_eq!(deduplicate(parse_jsonl(&duplicated)).len(), 2);
    }

    #[test]
    fn advisor_iteration_uses_its_own_model_price_alongside_parent_cost() {
        let content = r#"{"timestamp":"2026-07-15T08:00:00Z","requestId":"req_1","costUSD":0.001,"message":{"id":"msg_1","model":"main-model","usage":{"input_tokens":1,"output_tokens":2,"iterations":[{"type":"advisor_message","model":"claude-opus-4-6","input_tokens":10,"output_tokens":2}]}}}"#;
        let pricing = test_bundled_pricing();
        let advisor_cost = pricing
            .estimated_cost_dollars(
                "claude-opus-4-6",
                TokenBreakdown {
                    input: 10,
                    output: 2,
                    ..TokenBreakdown::default()
                },
                true,
            )
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();

        let history = aggregate(deduplicate(parse_jsonl(content)), now, &pricing);
        let today = history.today.unwrap();

        assert_eq!(today.tokens, 15);
        assert!((today.estimated_cost_usd.unwrap() - (0.001 + advisor_cost)).abs() < 1e-12);
        let models = today.model_breakdown.unwrap().models;
        assert_eq!(
            models
                .iter()
                .find(|entry| entry.model == "claude-opus-4-6")
                .map(|entry| entry.total_tokens),
            Some(12)
        );
    }

    #[test]
    fn parses_cross_device_timestamp_variants() {
        let content = r#"{"timestamp":"2026-07-15 12:00:00.123456 UTC","message":{"model":"claude-opus-4-6","usage":{"input_tokens":10,"output_tokens":5}}}"#;
        let event = parse_jsonl(content).pop().unwrap();
        assert_eq!(
            event.timestamp.to_rfc3339(),
            "2026-07-15T12:00:00.123+00:00"
        );
    }

    #[test]
    fn rejects_foreign_empty_and_null_schema_fields() {
        let missing_input =
            r#"{"timestamp":"2026-02-20T12:00:00Z","message":{"usage":{"output_tokens":5}}}"#;
        let invalid_version = r#"{"timestamp":"2026-02-20T12:00:00Z","version":"unknown","message":{"usage":{"input_tokens":1,"output_tokens":2}}}"#;
        let empty_model = r#"{"timestamp":"2026-02-20T12:00:00Z","message":{"model":"","usage":{"input_tokens":1,"output_tokens":2}}}"#;
        let null_speed = r#"{"timestamp":"2026-02-20T12:00:00Z","message":{"usage":{"input_tokens":1,"output_tokens":2,"speed":null}}}"#;
        let string_tokens = r#"{"timestamp":"2026-02-20T12:00:00Z","message":{"usage":{"input_tokens":"1","output_tokens":2}}}"#;
        for line in [missing_input, invalid_version, empty_model, string_tokens] {
            assert!(parse_line(line).is_none(), "unexpectedly accepted: {line}");
        }
        assert!(parse_jsonl(null_speed).is_empty());
        assert!(is_semver_prefix("1.0.24-beta.1"));
        assert!(!is_semver_prefix("1.0"));
        assert!(!has_unsupported_null_field(
            r#"{"message":{"content":null}}"#
        ));
        assert!(!has_unsupported_null_field(
            r#"{"message":{"usage":{"speed": null}}}"#
        ));
    }

    #[test]
    fn parent_replacement_removes_the_stale_dedup_key() {
        fn event(request_id: &str, sidechain: bool, cache_read: u64) -> ClaudeTokenEvent {
            ClaudeTokenEvent {
                timestamp: Utc.with_ymd_and_hms(2026, 2, 20, 12, 0, 0).unwrap(),
                model: Some("claude-opus-4-6".into()),
                input: 0,
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_read,
                output: 10,
                message_id: Some("msg-parent".into()),
                request_id: Some(request_id.into()),
                sidechain,
                is_fast: false,
                has_speed: false,
                cost_usd: None,
            }
        }
        let events = deduplicate(vec![
            event("req-sidechain", true, 50_000),
            event("req-parent", false, 20),
            event("req-sidechain", false, 5),
        ]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].request_id.as_deref(), Some("req-parent"));
        assert_eq!(events[0].cache_read, 20);
    }

    #[test]
    fn discovers_default_xdg_and_cowork_roots() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        let xdg = home.join(".config/claude");
        let standard = home.join(".claude");
        let cowork = home.join(
            "Library/Application Support/Claude/local-agent-mode-sessions/group/sub/local_1/.claude",
        );
        for root in [&xdg, &standard, &cowork] {
            fs::create_dir_all(root.join("projects")).unwrap();
        }
        let roots = claude_roots(None, None, home);
        assert_eq!(roots, vec![xdg, standard, cowork]);
    }

    #[test]
    fn config_override_supports_multiple_roots_projects_alias_and_cowork() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let first = home.join("claude-one");
        let second = directory.path().join("claude-two");
        let cowork = home.join(
            "Library/Application Support/Claude/local-agent-mode-sessions/group/sub/local_1/.claude",
        );
        fs::create_dir_all(first.join("projects")).unwrap();
        fs::create_dir_all(second.join("projects")).unwrap();
        fs::create_dir_all(cowork.join("projects")).unwrap();
        let configured = format!("~/claude-one,{}", second.join("projects").display());

        let roots = claude_roots(Some(&configured), None, &home);

        assert_eq!(roots, vec![first, second, cowork]);
    }

    #[test]
    fn discovers_logs_under_a_symlinked_projects_root() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("claude");
        let real_projects = directory.path().join("real-projects");
        let log = real_projects.join("project/session.jsonl");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(&log, "{}").unwrap();
        if create_directory_symlink(&real_projects, &root.join("projects")).is_err() {
            return;
        }

        assert_eq!(
            discover_files_in_roots(&[root]),
            vec![fs::canonicalize(log).unwrap()]
        );
    }

    #[test]
    fn account_scan_is_limited_to_its_configured_roots() {
        let directory = tempdir().unwrap();
        let account_root = directory.path().join("account");
        let other_root = directory.path().join("other");
        let account_log = account_root.join("projects/project/account.jsonl");
        let other_log = other_root.join("projects/project/other.jsonl");
        fs::create_dir_all(account_log.parent().unwrap()).unwrap();
        fs::create_dir_all(other_log.parent().unwrap()).unwrap();
        fs::write(&account_log, "{}").unwrap();
        fs::write(&other_log, "{}").unwrap();

        assert_eq!(
            discover_files(&[account_root], false),
            vec![fs::canonicalize(account_log).unwrap()]
        );
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[test]
    fn unknown_usage_is_excluded_but_remains_visible_as_incomplete() {
        let content = r#"{"timestamp":"2026-07-15T08:00:00Z","message":{"model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"output_tokens":10}}}
{"timestamp":"2026-07-15T09:00:00Z","message":{"model":"future-unpriced-model","usage":{"input_tokens":500,"output_tokens":20}}}"#;
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let history = aggregate(parse_jsonl(content), now, &test_bundled_pricing());
        let today = history.today.unwrap();
        assert_eq!(today.tokens, 110);
        assert!(!today.estimate_complete);
        assert_eq!(today.unknown_models, ["future-unpriced-model"]);
    }

    #[test]
    fn explicit_cost_wins_even_when_model_has_no_catalog_entry() {
        let content = r#"{"timestamp":"2026-07-15T08:00:00Z","costUSD":1.75,"message":{"model":"future-unpriced-model","usage":{"input_tokens":500,"output_tokens":20}}}"#;
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let history = aggregate(parse_jsonl(content), now, &test_bundled_pricing());
        let today = history.today.unwrap();
        assert_eq!(today.tokens, 520);
        assert_eq!(today.estimated_cost_usd, Some(1.75));
        assert!(today.estimate_complete);
        assert!(today.unknown_models.is_empty());
    }

    #[test]
    fn parser_preserves_supported_speed_for_fast_pricing() {
        let content = r#"{"timestamp":"2026-07-15T08:00:00Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":10,"speed":"fast"}}}"#;
        let event = parse_jsonl(content).pop().unwrap();
        assert!(event.is_fast);
        assert!(event.has_speed);
    }
}
