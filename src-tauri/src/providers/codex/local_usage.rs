use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{models::UsageHistory, pricing::ModelPricing, storage::Storage};

use super::CodexError;
use crate::providers::{
    daily_usage::DailyUsageAccumulator,
    log_usage::{load_or_parse_log, parse_log_timestamp, LogCacheError},
    pi_usage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TokenEvent {
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub reasoning: u64,
    pub total: u64,
}

const LOG_CACHE_SCHEMA_VERSION: u8 = 1;

pub fn scan_local_usage(
    storage: &Storage,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
) -> Result<UsageHistory, CodexError> {
    let home = home_directory();
    let configured_home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    let homes = codex_homes(configured_home.as_deref(), &home);
    let fast_tier = homes.iter().any(|home| uses_fast_service_tier(home));
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    let mut events = Vec::new();
    let paths = discover_session_files(&homes);
    let mut seen_paths = HashSet::with_capacity(paths.len());

    for path in paths {
        seen_paths.insert(path.clone());
        let Some(parsed) = load_or_parse_log(
            storage,
            "codex",
            &path,
            LOG_CACHE_SCHEMA_VERSION,
            parse_jsonl,
        )
        .map_err(|error| match error {
            LogCacheError::Storage(_) => CodexError::Storage,
            LogCacheError::Encode(_) => CodexError::LocalUsage,
        })?
        else {
            continue;
        };
        events.extend(
            parsed
                .into_iter()
                .filter(|event| event.timestamp.with_timezone(&Local).date_naive() >= since_date),
        );
    }
    storage.prune_log_events("codex", &seen_paths)?;

    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(events, now, fast_tier, pricing, &mut accumulator);
    let includes_pi = match pi_usage::scan_into(storage, now, pricing, "codex", &mut accumulator) {
        Ok(includes_pi) => includes_pi,
        Err(_) => {
            crate::app_warn!(
                "plugin:pi",
                "pi usage history could not be folded into Codex"
            );
            false
        }
    };
    let source_note = if includes_pi {
        "From your Codex logs and pi (estimated)"
    } else {
        "From your Codex logs (estimated)"
    };
    Ok(accumulator.build(now, source_note))
}

fn codex_homes(configured_home: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    if let Some(configured_home) = configured_home.filter(|path| !path.as_os_str().is_empty()) {
        return vec![configured_home.to_path_buf()];
    }
    vec![home.join(".codex")]
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn discover_session_files(homes: &[PathBuf]) -> Vec<PathBuf> {
    let mut output = Vec::new();
    let mut seen_directories = HashSet::new();
    for home in homes {
        let sources = [home.join("sessions"), home.join("archived_sessions")]
            .into_iter()
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        let sources = if sources.is_empty() {
            vec![home.clone()]
        } else {
            sources
        };
        let mut seen_relative = HashSet::new();
        for source in sources {
            let source = fs::canonicalize(&source).unwrap_or(source);
            if !seen_directories.insert(source.clone()) {
                continue;
            }
            let mut source_files = WalkDir::new(&source)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
                .map(|entry| entry.into_path())
                .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
                .collect::<Vec<_>>();
            source_files.sort();
            for path in source_files {
                let relative = path.strip_prefix(&source).unwrap_or(&path).to_path_buf();
                if seen_relative.insert(relative) {
                    output.push(path);
                }
            }
        }
    }
    output
}

fn uses_fast_service_tier(home: &Path) -> bool {
    let Ok(content) = fs::read_to_string(home.join("config.toml")) else {
        return false;
    };
    content.lines().any(|line| {
        let setting = line.split('#').next().unwrap_or_default();
        let Some((key, value)) = setting.split_once('=') else {
            return false;
        };
        key.trim() == "service_tier"
            && matches!(value.trim().trim_matches(['"', '\'']), "fast" | "priority")
    })
}

pub fn parse_jsonl(content: &str) -> Vec<TokenEvent> {
    let subagent = content.as_bytes()[..content.len().min(16 * 1024)]
        .windows("thread_spawn".len())
        .any(|window| window == b"thread_spawn");
    let replay_second = subagent.then(|| detect_replay_second(content)).flatten();
    let mut current_model: Option<String> = None;
    let mut previous_totals: Option<RawUsage> = None;
    let mut skip_replay = replay_second.is_some();
    let mut events = Vec::new();

    for line in content.lines() {
        if !line.contains("\"type\":\"turn_context\"") && !line.contains("\"type\":\"token_count\"")
        {
            continue;
        }
        let Ok(object) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(model) = model_name(object.get("payload")) {
                current_model = Some(model);
            }
            continue;
        }
        let Some(payload) = object.get("payload") else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) != Some("event_msg")
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
        {
            continue;
        }
        let Some(timestamp_raw) = object
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            continue;
        };
        let Some(timestamp) = parse_log_timestamp(timestamp_raw) else {
            continue;
        };
        let info = payload.get("info");
        let totals = info
            .and_then(|value| value.get("total_token_usage"))
            .map(RawUsage::from_value);

        if skip_replay {
            if replay_second.as_deref() == timestamp_raw.get(..19) {
                if let Some(totals) = totals {
                    previous_totals = Some(totals);
                }
                continue;
            }
            skip_replay = false;
        }

        let usage = if let Some(last) = info.and_then(|value| value.get("last_token_usage")) {
            RawUsage::from_value(last)
        } else if let Some(totals) = totals {
            totals.subtracting(previous_totals)
        } else {
            continue;
        };
        if let Some(totals) = totals {
            previous_totals = Some(totals);
        }
        if usage.input == 0 && usage.cached == 0 && usage.output == 0 && usage.reasoning == 0 {
            continue;
        }
        let parsed_model = model_name(Some(payload)).or_else(|| model_name(info));
        let model = resolve_model(parsed_model, timestamp_raw, &mut current_model);
        events.push(TokenEvent {
            timestamp,
            model,
            input: usage.input,
            cached: usage.cached.min(usage.input),
            output: usage.output,
            reasoning: usage.reasoning,
            total: usage.total,
        });
    }
    events
}

#[derive(Debug, Clone, Copy)]
struct RawUsage {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl RawUsage {
    fn from_value(value: &Value) -> Self {
        let input = integer(value, &["input_tokens", "prompt_tokens", "input"]);
        let cached = integer(
            value,
            &[
                "cached_input_tokens",
                "cache_read_input_tokens",
                "cached_tokens",
            ],
        );
        let output = integer(value, &["output_tokens", "completion_tokens", "output"]);
        let reasoning = integer(value, &["reasoning_output_tokens", "reasoning_tokens"]);
        let reported = integer(value, &["total_tokens"]);
        let recomputed = input + output + reasoning;
        Self {
            input,
            cached,
            output,
            reasoning,
            total: if reported > 0 || recomputed == 0 {
                reported
            } else {
                recomputed
            },
        }
    }

    fn subtracting(self, previous: Option<Self>) -> Self {
        let previous = previous.unwrap_or(Self {
            input: 0,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: 0,
        });
        Self {
            input: self.input.saturating_sub(previous.input),
            cached: self.cached.saturating_sub(previous.cached),
            output: self.output.saturating_sub(previous.output),
            reasoning: self.reasoning.saturating_sub(previous.reasoning),
            total: self.total.saturating_sub(previous.total),
        }
    }
}

fn integer(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or_default()
}

fn model_name(value: Option<&Value>) -> Option<String> {
    let value = value?;
    [
        value.get("model"),
        value.get("model_name"),
        value
            .get("metadata")
            .and_then(|metadata| metadata.get("model")),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn detect_replay_second(content: &str) -> Option<String> {
    let mut first = None;
    for line in content
        .lines()
        .filter(|line| line.contains("\"type\":\"token_count\""))
    {
        let Ok(object) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(payload) = object.get("payload") else {
            continue;
        };
        let Some(info) = payload.get("info") else {
            continue;
        };
        if object.get("type").and_then(Value::as_str) != Some("event_msg")
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
            || (info.get("last_token_usage").is_none() && info.get("total_token_usage").is_none())
        {
            continue;
        }
        let Some(timestamp) = object
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            continue;
        };
        let Some(second) = timestamp.get(..19).map(str::to_owned) else {
            continue;
        };
        if let Some(first) = first {
            return (first == second).then_some(second);
        }
        first = Some(second);
    }
    None
}

fn resolve_model(
    parsed: Option<String>,
    timestamp: &str,
    current_model: &mut Option<String>,
) -> String {
    if let Some(parsed) = parsed.as_ref() {
        *current_model = Some(parsed.clone());
    }
    let model = parsed.or_else(|| current_model.clone()).unwrap_or_else(|| {
        *current_model = Some("gpt-5".into());
        "gpt-5".into()
    });
    if model == "codex-auto-review" {
        auto_review_fallback(timestamp).to_owned()
    } else {
        model
    }
}

fn auto_review_fallback(timestamp: &str) -> &'static str {
    let date = timestamp.get(..10).unwrap_or_default();
    [
        ("2026-04-23", "gpt-5.5"),
        ("2026-03-05", "gpt-5.4"),
        ("2026-02-05", "gpt-5.3-codex"),
        ("2025-12-11", "gpt-5.2-codex"),
        ("2025-11-13", "gpt-5.1-codex"),
        ("2025-09-15", "gpt-5-codex"),
        ("2025-08-07", "gpt-5"),
    ]
    .into_iter()
    .find(|(released, _)| date >= *released)
    .map(|(_, model)| model)
    .unwrap_or("gpt-5")
}

#[cfg(test)]
fn aggregate(
    events: Vec<TokenEvent>,
    now: DateTime<Utc>,
    fast_tier: bool,
    pricing: &ModelPricing,
) -> UsageHistory {
    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(events, now, fast_tier, pricing, &mut accumulator);
    accumulator.build(now, "From your Codex logs (estimated)")
}

fn aggregate_into(
    events: Vec<TokenEvent>,
    now: DateTime<Utc>,
    fast_tier: bool,
    pricing: &ModelPricing,
    accumulator: &mut DailyUsageAccumulator,
) {
    let today = now.with_timezone(&Local).date_naive();
    let since = today.checked_sub_days(Days::new(30)).unwrap_or(today);
    let mut seen = HashSet::new();

    for event in events {
        if !seen.insert(event.clone()) {
            continue;
        }
        let date = event.timestamp.with_timezone(&Local).date_naive();
        if date < since {
            continue;
        }
        if let Some(cost) = estimate_cost(&event, fast_tier, pricing) {
            accumulator.add(date, event.total, cost, event.model.trim());
        } else if event.total > 0 {
            accumulator.add_unknown_model(date, &event.model);
        }
    }
}

fn estimate_cost(event: &TokenEvent, fast_tier: bool, pricing: &ModelPricing) -> Option<f64> {
    let rates = pricing.resolve(event.model.trim())?;
    let non_cached = event.input.saturating_sub(event.cached) as f64;
    let base_cost = (non_cached * rates.input_per_million
        + event.cached as f64 * rates.cache_read_per_million
        + event.output as f64 * rates.output_per_million)
        / 1_000_000.0;
    let multiplier = if fast_tier {
        if rates.fast_multiplier == 1.0 {
            2.0
        } else {
            rates.fast_multiplier
        }
    } else {
        1.0
    };
    Some(base_cost * multiplier)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, io, path::Path};

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::{
        aggregate, codex_homes, discover_session_files, estimate_cost, parse_jsonl, TokenEvent,
    };
    use crate::pricing::{
        test_bundled_pricing, ModelPricing, ModelRates, PricingCatalog, PricingSupplement,
    };

    #[test]
    fn parses_last_usage_and_tracks_turn_model() {
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"turn_context","payload":{"model":"gpt-5.5"}}
{"timestamp":"2026-07-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":115}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "gpt-5.5");
        assert_eq!(events[0].total, 115);
        assert_eq!(events[0].cached, 20);
    }

    #[test]
    fn cumulative_totals_become_deltas() {
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}
{"timestamp":"2026-07-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":160,"output_tokens":20,"total_tokens":180}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].input, 60);
        assert_eq!(events[1].output, 10);
    }

    #[test]
    fn auto_review_model_uses_the_event_date() {
        let content = r#"{"timestamp":"2026-03-10T08:00:00Z","type":"turn_context","payload":{"model":"codex-auto-review"}}
{"timestamp":"2026-03-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events[0].model, "gpt-5.4");
    }

    #[test]
    fn subagent_replay_seeds_the_cumulative_baseline() {
        let content = r#"{"timestamp":"2026-05-12T08:03:00Z","type":"session_meta","payload":{"source":{"subagent":{"thread_spawn":true}}}}
{"timestamp":"2026-05-12T08:03:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"total_tokens":1200}}}}
{"timestamp":"2026-05-12T08:03:00.500Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":150,"output_tokens":300,"total_tokens":1800}}}}
{"timestamp":"2026-05-12T08:04:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1600,"cached_input_tokens":160,"output_tokens":320,"total_tokens":1920}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input, 100);
        assert_eq!(events[0].cached, 10);
        assert_eq!(events[0].output, 20);
        assert_eq!(events[0].total, 120);
    }

    #[test]
    fn accepts_trimmed_timestamps_and_rejects_numeric_strings() {
        let content = r#"{"timestamp":" 2026-07-10T08:00:00Z ","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}
{"timestamp":"2026-07-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":"100","output_tokens":"10","total_tokens":"110"}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].total, 110);
    }

    #[test]
    fn parses_cross_device_timestamp_offsets() {
        let content = r#"{"timestamp":"2026-07-15 15:00:00.123456+03:00","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(
            events[0].timestamp.to_rfc3339(),
            "2026-07-15T12:00:00.123+00:00"
        );
    }

    #[test]
    fn codex_home_override_is_one_exact_path_even_when_it_contains_a_comma() {
        let directory = tempdir().unwrap();
        let default_home = directory.path().join("home");
        let configured = directory.path().join("codex,work");

        assert_eq!(
            codex_homes(Some(&configured), &default_home),
            vec![configured]
        );
        assert_eq!(
            codex_homes(None, &default_home),
            vec![default_home.join(".codex")]
        );
    }

    #[test]
    fn active_sessions_win_over_matching_archived_paths() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        let relative = "2026/07/rollout.jsonl";
        let active = home.join("sessions").join(relative);
        let archived = home.join("archived_sessions").join(relative);
        fs::create_dir_all(active.parent().unwrap()).unwrap();
        fs::create_dir_all(archived.parent().unwrap()).unwrap();
        fs::write(&active, "active").unwrap();
        fs::write(&archived, "archived").unwrap();

        assert_eq!(
            discover_session_files(&[home.to_path_buf()]),
            vec![fs::canonicalize(active).unwrap()]
        );
    }

    #[test]
    fn discovers_logs_under_a_symlinked_sessions_root() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("codex");
        let real_sessions = directory.path().join("real-sessions");
        let log = real_sessions.join("2026/07/rollout.jsonl");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(&log, "{}").unwrap();
        if create_directory_symlink(&real_sessions, &home.join("sessions")).is_err() {
            return;
        }

        assert_eq!(
            discover_session_files(&[home]),
            vec![fs::canonicalize(log).unwrap()]
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
    fn newly_priced_models_produce_a_complete_estimate() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.6-sol","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#;
        let pricing = test_bundled_pricing();
        let history = aggregate(parse_jsonl(content), now, false, &pricing);
        assert_eq!(history.today.as_ref().unwrap().tokens, 110);
        assert!(history.today.as_ref().unwrap().estimated_cost_usd.is_some());
        assert!(history.today.as_ref().unwrap().estimate_complete);
        assert!(history.unknown_models.is_empty());
    }

    #[test]
    fn period_breakdown_uses_model_names_and_excludes_unpriced_usage() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":1000,"output_tokens":100,"total_tokens":1100}}}}
{"timestamp":"2026-07-10T09:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.3-codex","info":{"last_token_usage":{"input_tokens":800,"output_tokens":100,"total_tokens":900}}}}
{"timestamp":"2026-07-10T10:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"future-unpriced-model","info":{"last_token_usage":{"input_tokens":400,"output_tokens":100,"total_tokens":500}}}}"#;
        let pricing = test_bundled_pricing();
        let history = aggregate(parse_jsonl(content), now, false, &pricing);
        let today = history.today.unwrap();
        let breakdown = today.model_breakdown.unwrap();

        assert_eq!(today.tokens, 2_000);
        assert_eq!(today.unknown_models, ["future-unpriced-model"]);
        assert_eq!(
            breakdown
                .models
                .iter()
                .map(|entry| entry.model.as_str())
                .collect::<Vec<_>>(),
            ["gpt-5.4", "gpt-5.3-codex"]
        );
        assert_eq!(breakdown.source_note, "From your Codex logs (estimated)");
    }

    #[test]
    fn unknown_only_usage_does_not_create_spend_periods() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T10:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"future-unpriced-model","info":{"last_token_usage":{"input_tokens":400,"output_tokens":100,"total_tokens":500}}}}"#;
        let pricing = test_bundled_pricing();
        let history = aggregate(parse_jsonl(content), now, false, &pricing);

        assert!(history.today.is_none());
        assert!(history.last_30_days.is_none());
        assert!(history.daily.is_empty());
        assert_eq!(history.unknown_models, ["future-unpriced-model"]);
    }

    #[test]
    fn provider_fixture_parses_realistic_codex_jsonl() {
        let content = include_str!("../../../tests/fixtures/codex_session.jsonl");
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 2);
        assert_eq!(events.iter().map(|event| event.total).sum::<u64>(), 225);
        assert!(events.iter().all(|event| event.model == "gpt-5.4"));
    }

    #[test]
    fn fast_tier_defaults_to_two_x_multiplier() {
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("test-model".into(), ModelRates::new(2.0, 8.0))]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "test-model".into(),
            input: 1_000_000,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: 1_000_000,
        };
        assert_eq!(estimate_cost(&event, false, &pricing), Some(2.0));
        assert_eq!(estimate_cost(&event, true, &pricing), Some(4.0));
    }
}
