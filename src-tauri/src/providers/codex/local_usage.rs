use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{
    models::UsageHistory,
    pricing::{ModelPricing, ModelRates, TokenBreakdown},
    storage::Storage,
};

use super::CodexError;
use crate::providers::{
    daily_usage::DailyUsageAccumulator,
    log_usage::{load_or_parse_log, parse_log_timestamp, LogCacheError},
    pi_usage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenEvent {
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub reasoning: u64,
    pub total: u64,
    pub is_fast: bool,
}

const LOG_CACHE_SCHEMA_VERSION: u8 = 3;

pub fn scan_local_usage_scoped(
    storage: &Storage,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    provider_id: &str,
    session_roots: &[PathBuf],
) -> Result<UsageHistory, CodexError> {
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    let events = scan_codex_events(storage, provider_id, session_roots, since_date)?;

    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(events, now, pricing, &mut accumulator);
    let includes_pi =
        match pi_usage::scan_into(storage, now, pricing, provider_id, &mut accumulator) {
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

fn scan_codex_events(
    storage: &Storage,
    provider_id: &str,
    homes: &[PathBuf],
    since_date: NaiveDate,
) -> Result<Vec<TokenEvent>, CodexError> {
    let mut events = Vec::new();
    let paths = discover_session_files(homes);
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
    storage.prune_log_events(provider_id, &seen_paths)?;
    Ok(events)
}

fn codex_homes(configured_home: Option<&OsStr>, home: &Path) -> Vec<PathBuf> {
    if let Some(configured_home) = configured_home.filter(|value| !value.is_empty()) {
        let configured_home = configured_home.to_string_lossy();
        if !configured_home.trim().is_empty() {
            return configured_home
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| expand_home(value, home))
                .collect();
        }
    }
    vec![home.join(".codex")]
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home.join(relative);
    }
    PathBuf::from(value)
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

pub fn parse_jsonl(content: &str) -> Vec<TokenEvent> {
    let mut current_model: Option<String> = None;
    let mut current_tier_is_fast = false;
    let mut previous_totals: Option<RawUsage> = None;
    let mut saw_session_meta = false;
    let mut replay_gate: Option<ChildReplayGate> = None;
    let mut events = Vec::new();

    for line in content.lines() {
        let is_turn_context = line.contains("\"type\":\"turn_context\"");
        let is_session_meta = !saw_session_meta && line.contains("\"type\":\"session_meta\"");
        let is_task_started = replay_gate.is_some() && line.contains("\"type\":\"task_started\"");
        let is_thread_settings = line.contains("\"type\":\"thread_settings_applied\"");
        if !is_turn_context
            && !is_session_meta
            && !is_task_started
            && !is_thread_settings
            && !line.contains("\"type\":\"token_count\"")
        {
            continue;
        }
        let Ok(object) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let object_type = object.get("type").and_then(Value::as_str);
        let payload = object.get("payload");

        if object_type == Some("turn_context") {
            if let Some(model) = model_name(object.get("payload")) {
                current_model = Some(model);
            }
            continue;
        }

        if object_type == Some("session_meta") && !saw_session_meta {
            saw_session_meta = true;
            if payload.is_some_and(is_child_session_meta) {
                replay_gate = Some(
                    object
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .and_then(parse_log_timestamp)
                        .map(|timestamp| ChildReplayGate::UntilStartedAt(timestamp.timestamp()))
                        .unwrap_or(ChildReplayGate::UntilSelfTimedTaskStarted),
                );
            }
            continue;
        }

        let Some(payload) = payload else {
            continue;
        };
        if object_type != Some("event_msg") {
            continue;
        }

        let payload_type = payload.get("type").and_then(Value::as_str);
        if payload_type == Some("thread_settings_applied") {
            if let Some(tier) = service_tier(payload) {
                current_tier_is_fast = matches!(tier, "fast" | "priority");
            }
            continue;
        }

        if payload_type == Some("task_started") {
            if replay_gate.is_some_and(|gate| gate.is_cleared(payload, object.get("timestamp"))) {
                replay_gate = None;
            }
            continue;
        }

        if payload_type != Some("token_count") {
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

        if replay_gate.is_some() {
            if let Some(totals) = totals {
                previous_totals = Some(totals);
            }
            continue;
        }

        if totals.is_some_and(|totals| previous_totals == Some(totals)) {
            continue;
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
        let model = resolve_model(parsed_model, &mut current_model);
        events.push(TokenEvent {
            timestamp,
            model,
            input: usage.input,
            cached: usage.cached.min(usage.input),
            output: usage.output,
            reasoning: usage.reasoning,
            total: usage.total,
            is_fast: current_tier_is_fast,
        });
    }
    events
}

fn service_tier(payload: &Value) -> Option<&str> {
    [
        payload
            .get("thread_settings")
            .and_then(|settings| settings.get("service_tier")),
        payload.get("service_tier"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

#[derive(Debug, Clone, Copy)]
enum ChildReplayGate {
    UntilStartedAt(i64),
    UntilSelfTimedTaskStarted,
}

impl ChildReplayGate {
    fn is_cleared(self, payload: &Value, line_timestamp: Option<&Value>) -> bool {
        let Some(started_at) = payload.get("started_at").and_then(Value::as_f64) else {
            return false;
        };
        match self {
            Self::UntilStartedAt(gate) => started_at >= gate as f64,
            Self::UntilSelfTimedTaskStarted => line_timestamp
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(parse_log_timestamp)
                .is_some_and(|timestamp| started_at >= timestamp.timestamp() as f64),
        }
    }
}

fn is_child_session_meta(payload: &Value) -> bool {
    has_non_null_value(payload.get("forked_from_id"))
        || has_non_null_value(payload.get("parent_thread_id"))
        || payload.get("thread_source").and_then(Value::as_str) == Some("subagent")
        || payload
            .get("source")
            .and_then(|source| source.get("subagent"))
            .is_some_and(|value| has_non_null_value(Some(value)))
}

fn has_non_null_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(_) => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

fn resolve_model(parsed: Option<String>, current_model: &mut Option<String>) -> String {
    if let Some(parsed) = parsed.as_ref() {
        *current_model = Some(parsed.clone());
    }
    parsed.or_else(|| current_model.clone()).unwrap_or_else(|| {
        *current_model = Some("gpt-5".into());
        "gpt-5".into()
    })
}

fn auto_review_fallback(timestamp: &DateTime<Utc>) -> &'static str {
    let date = timestamp.date_naive();
    [
        ((2026, 4, 23), "gpt-5.5"),
        ((2026, 3, 5), "gpt-5.4"),
        ((2026, 2, 5), "gpt-5.3-codex"),
        ((2025, 12, 11), "gpt-5.2-codex"),
        ((2025, 11, 13), "gpt-5.1-codex"),
        ((2025, 9, 15), "gpt-5-codex"),
        ((2025, 8, 7), "gpt-5"),
    ]
    .into_iter()
    .find(|((year, month, day), _)| {
        date >= NaiveDate::from_ymd_opt(*year, *month, *day).expect("valid release date")
    })
    .map(|(_, model)| model)
    .unwrap_or("gpt-5")
}

#[cfg(test)]
fn aggregate(events: Vec<TokenEvent>, now: DateTime<Utc>, pricing: &ModelPricing) -> UsageHistory {
    let mut accumulator = DailyUsageAccumulator::default();
    aggregate_into(events, now, pricing, &mut accumulator);
    accumulator.build(now, "From your Codex logs (estimated)")
}

fn aggregate_into(
    events: Vec<TokenEvent>,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    accumulator: &mut DailyUsageAccumulator,
) {
    let today = now.with_timezone(&Local).date_naive();
    let since = today.checked_sub_days(Days::new(30)).unwrap_or(today);
    let mut seen = HashSet::new();

    for event in events {
        let key = (
            event.timestamp,
            event.model.clone(),
            event.input,
            event.cached,
            event.output,
            event.reasoning,
            event.total,
        );
        if !seen.insert(key) {
            continue;
        }
        let date = event.timestamp.with_timezone(&Local).date_naive();
        if date < since {
            continue;
        }
        if let Some(cost) = estimate_cost(&event, pricing) {
            accumulator.add(date, event.total, cost, event.model.trim());
        } else if event.total > 0 {
            accumulator.add_unknown_model(date, &event.model);
        }
    }
}

fn estimate_cost(event: &TokenEvent, pricing: &ModelPricing) -> Option<f64> {
    let display_model = event.model.trim();
    let model = if display_model == "codex-auto-review" {
        auto_review_fallback(&event.timestamp)
    } else {
        display_model
    };
    let canonical = pricing.supplement.canonical_name(model).unwrap_or(model);
    let fast_base = canonical
        .strip_suffix("-fast")
        .filter(|base| !base.is_empty());
    let rate_model = fast_base.unwrap_or(canonical);
    let base_rates = pricing.resolve(rate_model);
    let rates = base_rates.or_else(|| pricing.resolve(model))?;
    let applies_fast_tier = if fast_base.is_some() {
        base_rates.is_some()
    } else {
        event.is_fast
    };
    Some(codex_cost(rates, event, rate_model, applies_fast_tier))
}

fn codex_cost(mut rates: ModelRates, event: &TokenEvent, model: &str, fast_tier: bool) -> f64 {
    if let Some((input, output, cache_read)) = codex_long_context_rates(model) {
        rates.input_above_200k_per_million = Some(input);
        rates.output_above_200k_per_million = Some(output);
        rates.cache_read_above_200k_per_million = Some(cache_read);
        rates.long_context_threshold_tokens = 272_000;
    }
    if codex_model_has_no_cache_discount(model) || !rates.cache_read_is_explicit {
        rates.cache_read_per_million = rates.input_per_million;
        rates.cache_read_above_200k_per_million = rates.input_above_200k_per_million;
    }
    rates.fast_multiplier = codex_priority_multiplier(model, rates);
    rates.cost_dollars(
        TokenBreakdown {
            input: event.input.saturating_sub(event.cached),
            cache_read: event.cached,
            output: event.output,
            is_fast: fast_tier,
            ..TokenBreakdown::default()
        },
        true,
    )
}

fn codex_priority_multiplier(model: &str, rates: ModelRates) -> f64 {
    match dated_base_model(model) {
        "gpt-5.5" | "gpt-5.5-pro" => 2.5,
        "gpt-5.4" | "gpt-5.4-pro" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" => 2.0,
        _ if rates.fast_multiplier == 1.0 => 2.0,
        _ => rates.fast_multiplier,
    }
}

fn codex_model_has_no_cache_discount(model: &str) -> bool {
    matches!(dated_base_model(model), "gpt-5.4-pro" | "gpt-5.5-pro")
}

fn codex_long_context_rates(model: &str) -> Option<(f64, f64, f64)> {
    match dated_base_model(model) {
        "gpt-5.4" => Some((5.0, 22.5, 0.5)),
        "gpt-5.4-pro" => Some((60.0, 270.0, 60.0)),
        "gpt-5.5" => Some((10.0, 45.0, 1.0)),
        "gpt-5.5-pro" => Some((60.0, 270.0, 60.0)),
        "gpt-5.6-sol" => Some((10.0, 45.0, 1.0)),
        "gpt-5.6-terra" => Some((4.0, 18.0, 0.4)),
        "gpt-5.6-luna" => Some((0.4, 1.8, 0.04)),
        _ => None,
    }
}

fn dated_base_model(model: &str) -> &str {
    let bytes = model.as_bytes();
    if bytes.len() >= 11 {
        let suffix = &bytes[bytes.len() - 11..];
        if suffix[0] == b'-'
            && suffix[1..5].iter().all(u8::is_ascii_digit)
            && suffix[5] == b'-'
            && suffix[6..8].iter().all(u8::is_ascii_digit)
            && suffix[8] == b'-'
            && suffix[9..11].iter().all(u8::is_ascii_digit)
        {
            return &model[..model.len() - 11];
        }
    }
    if bytes.len() >= 9 {
        let suffix = &bytes[bytes.len() - 9..];
        if suffix[0] == b'-' && suffix[1..].iter().all(u8::is_ascii_digit) {
            return &model[..model.len() - 9];
        }
    }
    model
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, io, path::Path};

    use chrono::{NaiveDate, TimeZone, Utc};
    use tempfile::tempdir;

    use super::{
        aggregate, codex_homes, codex_long_context_rates, codex_priority_multiplier,
        discover_session_files, estimate_cost, parse_jsonl, scan_codex_events, TokenEvent,
    };
    use crate::{
        pricing::{
            test_bundled_pricing, ModelPricing, ModelRates, PricingCatalog, PricingSupplement,
        },
        providers::log_usage::LogFileFingerprint,
        storage::Storage,
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
    fn auto_review_keeps_its_model_name() {
        let content = r#"{"timestamp":"2026-03-10T08:00:00Z","type":"turn_context","payload":{"model":"codex-auto-review"}}
{"timestamp":"2026-03-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events[0].model, "codex-auto-review");
    }

    #[test]
    fn auto_review_uses_fallback_rates_but_keeps_its_breakdown_label() {
        let now = Utc.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-03-10T08:00:00Z","type":"turn_context","payload":{"model":"codex-auto-review"}}
{"timestamp":"2026-03-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100000,"output_tokens":100000,"total_tokens":200000}}}}"#;
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("gpt-5.4".into(), ModelRates::new(2.0, 8.0))]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );

        let history = aggregate(parse_jsonl(content), now, &pricing);
        let today = history.today.unwrap();
        let breakdown = today.model_breakdown.unwrap();

        assert_eq!(today.estimated_cost_usd, Some(1.0));
        assert!(today.unknown_models.is_empty());
        assert_eq!(breakdown.models.len(), 1);
        assert_eq!(breakdown.models[0].model, "codex-auto-review");
        assert_eq!(breakdown.models[0].cost_usd, Some(1.0));
    }

    #[test]
    fn subagent_replay_seeds_the_cumulative_baseline() {
        let content = r#"{"timestamp":"2026-05-12T08:03:00Z","type":"session_meta","payload":{"source":{"subagent":{"thread_spawn":true}}}}
{"timestamp":"2026-05-12T08:03:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"total_tokens":1200}}}}
{"timestamp":"2026-05-12T08:04:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":150,"output_tokens":300,"total_tokens":1800}}}}
{"timestamp":"2026-05-12T08:04:30Z","type":"event_msg","payload":{"type":"task_started","started_at":1}}
{"timestamp":"2026-05-12T08:05:00Z","type":"event_msg","payload":{"type":"task_started","started_at":9999999999}}
{"timestamp":"2026-05-12T08:06:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1600,"cached_input_tokens":160,"output_tokens":320,"total_tokens":1920}}}}"#;
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input, 100);
        assert_eq!(events[0].cached, 10);
        assert_eq!(events[0].output, 20);
        assert_eq!(events[0].total, 120);
    }

    #[test]
    fn child_without_a_live_task_emits_no_replayed_usage() {
        let content = r#"{"timestamp":"2026-05-12T08:03:00Z","type":"session_meta","payload":{"forked_from_id":"parent"}}
{"timestamp":"2026-05-12T08:04:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120},"total_token_usage":{"input_tokens":1000,"output_tokens":200,"total_tokens":1200}}}}"#;
        assert!(parse_jsonl(content).is_empty());
    }

    #[test]
    fn child_without_a_metadata_timestamp_opens_only_on_its_live_task() {
        let content = r#"{"type":"session_meta","payload":{"forked_from_id":"parent"}}
{"timestamp":"2026-05-12T08:03:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"output_tokens":200,"total_tokens":1200}}}}
{"timestamp":"2026-05-12T08:03:30Z","type":"event_msg","payload":{"type":"task_started","started_at":1}}
{"timestamp":"2026-05-12T08:04:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1500,"output_tokens":300,"total_tokens":1800}}}}
{"timestamp":"2026-05-12T08:05:00Z","type":"event_msg","payload":{"type":"task_started","started_at":9999999999}}
{"timestamp":"2026-05-12T08:06:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1600,"output_tokens":320,"total_tokens":1920}}}}"#;
        let events = parse_jsonl(content);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input, 100);
        assert_eq!(events[0].output, 20);
        assert_eq!(events[0].total, 120);
    }

    #[test]
    fn null_parent_metadata_keeps_root_session_usage() {
        let content = r#"{"timestamp":"2026-05-12T08:03:00Z","type":"session_meta","payload":{"forked_from_id":null,"parent_thread_id":" "}}
{"timestamp":"2026-05-12T08:04:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}}"#;
        assert_eq!(parse_jsonl(content).len(), 1);
    }

    #[test]
    fn service_tier_is_attached_to_each_usage_event() {
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"service_tier":" fast "}}}
{"timestamp":"2026-07-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}
{"timestamp":"2026-07-10T08:02:00Z","type":"event_msg","payload":{"type":"thread_settings_applied","service_tier":"default"}}
{"timestamp":"2026-07-10T08:03:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"output_tokens":5,"total_tokens":55}}}}"#;
        let events = parse_jsonl(content);
        assert!(events[0].is_fast);
        assert!(!events[1].is_fast);
    }

    #[test]
    fn unchanged_cumulative_snapshot_is_not_counted_twice() {
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110},"total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}
{"timestamp":"2026-07-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110},"total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#;
        assert_eq!(parse_jsonl(content).len(), 1);
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
    fn codex_home_override_supports_multiple_comma_separated_paths() {
        let directory = tempdir().unwrap();
        let default_home = directory.path().join("home");
        let first = directory.path().join("codex-work");
        let second = directory.path().join("codex-personal");
        let configured = format!("{}, {}", first.display(), second.display());

        assert_eq!(
            codex_homes(Some(configured.as_ref()), &default_home),
            vec![first, second]
        );
        assert_eq!(
            codex_homes(Some("~/.codex-alt".as_ref()), &default_home),
            vec![default_home.join(".codex-alt")]
        );
        assert_eq!(
            codex_homes(None, &default_home),
            vec![default_home.join(".codex")]
        );
        assert_eq!(
            codex_homes(Some("  \t ".as_ref()), &default_home),
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

    #[test]
    fn scan_cache_picks_up_changed_and_new_logs_without_using_config_tier() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("codex");
        let sessions = home.join("sessions");
        let first = sessions.join("rollout-a.jsonl");
        let second = sessions.join("rollout-b.jsonl");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(home.join("config.toml"), "service_tier = \"priority\"").unwrap();
        fs::write(
            &first,
            r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#,
        )
        .unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let since = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();

        let initial =
            scan_codex_events(&storage, "codex", std::slice::from_ref(&home), since).unwrap();
        assert_eq!(initial.len(), 1);
        assert!(!initial[0].is_fast);

        fs::write(
            &first,
            r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":130,"output_tokens":10,"total_tokens":140}}}}"#,
        )
        .unwrap();
        fs::write(
            &second,
            r#"{"timestamp":"2026-07-10T09:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.5","info":{"last_token_usage":{"input_tokens":50,"output_tokens":5,"total_tokens":55}}}}"#,
        )
        .unwrap();

        let refreshed = scan_codex_events(&storage, "codex", &[home], since).unwrap();
        assert_eq!(refreshed.len(), 2);
        assert_eq!(refreshed.iter().map(|event| event.total).sum::<u64>(), 195);
        assert!(refreshed.iter().all(|event| !event.is_fast));
    }

    #[test]
    fn schema_upgrade_reparses_cached_auto_review_events() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("codex");
        let sessions = home.join("sessions");
        let path = sessions.join("rollout.jsonl");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            &path,
            r#"{"timestamp":"2026-03-10T08:00:00Z","type":"turn_context","payload":{"model":"codex-auto-review"}}
{"timestamp":"2026-03-10T08:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}}"#,
        )
        .unwrap();
        let path = fs::canonicalize(path).unwrap();
        let fingerprint = LogFileFingerprint::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        storage
            .save_log_events(
                "codex",
                &path,
                fingerprint.size,
                fingerprint.modified_nanos,
                r#"{"schema_version":2,"events":[{"timestamp":"2026-03-10T08:01:00Z","model":"gpt-5.4","input":10,"cached":0,"output":5,"reasoning":0,"total":15,"is_fast":false}]}"#,
            )
            .unwrap();

        let events = scan_codex_events(
            &storage,
            "codex",
            &[home],
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        )
        .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "codex-auto-review");
        let cached = storage
            .load_log_events("codex", &path, fingerprint.size, fingerprint.modified_nanos)
            .unwrap()
            .unwrap();
        let cached = serde_json::from_str::<serde_json::Value>(&cached).unwrap();
        assert_eq!(cached["schema_version"], 3);
        assert_eq!(cached["events"][0]["model"], "codex-auto-review");
        assert!(cached["events"][0].get("pricing_model").is_none());
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
        let history = aggregate(parse_jsonl(content), now, &pricing);
        assert_eq!(history.today.as_ref().unwrap().tokens, 110);
        assert!(history.today.as_ref().unwrap().estimated_cost_usd.is_some());
        assert!(history.today.as_ref().unwrap().estimate_complete);
        assert!(history.unknown_models.is_empty());
    }

    #[test]
    fn daybreak_uses_sol_rates_without_losing_its_breakdown_identity() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-daybreak-blue-latest","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#;
        let pricing = test_bundled_pricing();
        let history = aggregate(parse_jsonl(content), now, &pricing);
        let today = history.today.as_ref().unwrap();
        assert_eq!(today.tokens, 110);
        assert_eq!(today.estimated_cost_usd, Some(0.0008));
        assert!(today.estimate_complete);
        assert_eq!(
            today.model_breakdown.as_ref().unwrap().models[0].model,
            "gpt-daybreak-blue-latest"
        );
        assert!(history.unknown_models.is_empty());
    }

    #[test]
    fn period_breakdown_uses_model_names_and_excludes_unpriced_usage() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":1000,"output_tokens":100,"total_tokens":1100}}}}
{"timestamp":"2026-07-10T09:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.3-codex","info":{"last_token_usage":{"input_tokens":800,"output_tokens":100,"total_tokens":900}}}}
{"timestamp":"2026-07-10T10:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"future-unpriced-model","info":{"last_token_usage":{"input_tokens":400,"output_tokens":100,"total_tokens":500}}}}"#;
        let pricing = test_bundled_pricing();
        let history = aggregate(parse_jsonl(content), now, &pricing);
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
        let history = aggregate(parse_jsonl(content), now, &pricing);

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
    fn event_fast_tier_defaults_to_two_x_multiplier() {
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
            is_fast: false,
        };
        assert_eq!(estimate_cost(&event, &pricing), Some(2.0));
        assert_eq!(
            estimate_cost(
                &TokenEvent {
                    is_fast: true,
                    ..event
                },
                &pricing
            ),
            Some(4.0)
        );
    }

    #[test]
    fn missing_cache_rate_provenance_uses_full_input_price() {
        let mut inferred_rates = ModelRates::new(2.0, 8.0);
        inferred_rates.cache_read_is_explicit = false;
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("test-model".into(), inferred_rates)]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "test-model".into(),
            input: 1_000_000,
            cached: 1_000_000,
            output: 0,
            reasoning: 0,
            total: 1_000_000,
            is_fast: false,
        };

        assert_eq!(estimate_cost(&event, &pricing), Some(2.0));
    }

    #[test]
    fn pro_model_overrides_an_explicit_legacy_cache_discount() {
        let mut rates = ModelRates::new(2.0, 8.0);
        rates.cache_read_per_million = 0.2;
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("gpt-5.5-pro".into(), rates)]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.5-pro".into(),
            input: 100_000,
            cached: 100_000,
            output: 0,
            reasoning: 0,
            total: 100_000,
            is_fast: false,
        };

        assert_eq!(estimate_cost(&event, &pricing), Some(0.2));
    }

    #[test]
    fn codex_long_context_rates_start_above_272k_prompt_tokens() {
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("gpt-5.4".into(), ModelRates::new(2.5, 15.0))]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let event = |input| TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.4".into(),
            input,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: input,
            is_fast: false,
        };

        assert!((estimate_cost(&event(272_000), &pricing).unwrap() - 0.68).abs() < 0.000_001);
        assert!((estimate_cost(&event(272_001), &pricing).unwrap() - 1.360_005).abs() < 0.000_001);
    }

    #[test]
    fn codex_long_context_rate_matrix_matches_supported_model_families() {
        for (model, expected) in [
            ("gpt-5.4", (5.0, 22.5, 0.5)),
            ("gpt-5.4-pro", (60.0, 270.0, 60.0)),
            ("gpt-5.5", (10.0, 45.0, 1.0)),
            ("gpt-5.5-pro", (60.0, 270.0, 60.0)),
            ("gpt-5.6-sol", (10.0, 45.0, 1.0)),
            ("gpt-5.6-terra", (4.0, 18.0, 0.4)),
            ("gpt-5.6-luna", (0.4, 1.8, 0.04)),
        ] {
            assert_eq!(codex_long_context_rates(model), Some(expected), "{model}");
        }
        assert_eq!(
            codex_long_context_rates("gpt-5.5-2026-04-23"),
            Some((10.0, 45.0, 1.0))
        );
        assert_eq!(codex_long_context_rates("gpt-5.3-codex"), None);
    }

    #[test]
    fn codex_priority_multiplier_matrix_overrides_generic_fast_prices() {
        let mut catalog_rates = ModelRates::new(2.0, 8.0);
        catalog_rates.fast_multiplier = 3.0;

        for model in ["gpt-5.5", "gpt-5.5-pro-20260423"] {
            assert_eq!(codex_priority_multiplier(model, catalog_rates), 2.5);
        }
        for model in [
            "gpt-5.4",
            "gpt-5.4-pro",
            "gpt-5.6-sol",
            "gpt-5.6-terra-2026-07-01",
            "gpt-5.6-luna",
        ] {
            assert_eq!(
                codex_priority_multiplier(model, catalog_rates),
                2.0,
                "{model}"
            );
        }
        assert_eq!(
            codex_priority_multiplier("future-model", catalog_rates),
            3.0
        );
        assert_eq!(
            codex_priority_multiplier("future-model", ModelRates::new(2.0, 8.0)),
            2.0
        );
    }

    #[test]
    fn fast_alias_uses_unscaled_base_rates_and_one_codex_multiplier() {
        let supplement = PricingSupplement::decode(
            br#"{"pricing":{},"fast_multipliers":{"gpt-5.5":2.5},"alias_rules":[]}"#,
        )
        .unwrap();
        let pricing = ModelPricing::new(
            supplement,
            PricingCatalog {
                entries: HashMap::from([("gpt-5.5".into(), ModelRates::new(2.0, 8.0))]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.5-fast".into(),
            input: 100_000,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: 100_000,
            is_fast: true,
        };

        assert_eq!(estimate_cost(&event, &pricing), Some(0.5));
    }

    #[test]
    fn fast_only_catalog_model_keeps_its_existing_rate_without_a_second_multiplier() {
        let pricing = ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog::default(),
            PricingCatalog {
                entries: HashMap::from([("vendor-model-fast".into(), ModelRates::new(6.0, 20.0))]),
                retrieved_at: None,
            },
        );
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "vendor-model-fast".into(),
            input: 100_000,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: 100_000,
            is_fast: true,
        };

        assert_eq!(estimate_cost(&event, &pricing), Some(0.6));
    }
}
