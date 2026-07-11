use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{
    models::{DailyUsage, UsageHistory, UsagePeriod},
    storage::Storage,
};

use super::{pricing::estimate_cost, CodexError};

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

#[derive(Default)]
struct DayAccumulator {
    tokens: u64,
    cost: f64,
    priced_events: usize,
    unpriced_events: usize,
    unknown_models: HashSet<String>,
}

pub fn scan_local_usage(storage: &Storage, now: DateTime<Utc>) -> Result<UsageHistory, CodexError> {
    let homes = codex_homes();
    let fast_tier = homes.iter().any(|home| uses_fast_service_tier(home));
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(29))
        .unwrap_or(NaiveDate::MIN);
    let mut events = Vec::new();

    for path in discover_session_files(&homes) {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_millis() as i64)
            .unwrap_or_default();
        let cached = storage.load_log_events(&path, metadata.len(), modified_millis)?;
        let parsed = if let Some(parsed) = cached
            .as_deref()
            .and_then(|json| serde_json::from_str::<Vec<TokenEvent>>(json).ok())
        {
            parsed
        } else {
            let parsed = parse_path(&path);
            let json = serde_json::to_string(&parsed).map_err(|_| CodexError::LocalUsage)?;
            storage.save_log_events(&path, metadata.len(), modified_millis, &json)?;
            parsed
        };
        events.extend(
            parsed
                .into_iter()
                .filter(|event| event.timestamp.with_timezone(&Local).date_naive() >= since_date),
        );
    }

    Ok(aggregate(events, now, fast_tier))
}

fn codex_homes() -> Vec<PathBuf> {
    if let Some(raw) = std::env::var_os("CODEX_HOME") {
        let homes = raw
            .to_string_lossy()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(expand_home)
            .collect::<Vec<_>>();
        if !homes.is_empty() {
            return homes;
        }
    }
    vec![home_directory().join(".codex")]
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return home_directory();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home_directory().join(rest);
    }
    PathBuf::from(value)
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
            if !seen_directories.insert(source.clone()) {
                continue;
            }
            for entry in WalkDir::new(&source)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
            {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                    continue;
                }
                let relative = path.strip_prefix(&source).unwrap_or(path).to_path_buf();
                if seen_relative.insert(relative) {
                    output.push(path.to_path_buf());
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

fn parse_path(path: &Path) -> Vec<TokenEvent> {
    fs::read_to_string(path)
        .map(|content| parse_jsonl(&content))
        .unwrap_or_default()
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
        if !line.contains("\"turn_context\"") && !line.contains("\"token_count\"") {
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
        let Some(timestamp_raw) = object.get("timestamp").and_then(Value::as_str) else {
            continue;
        };
        let Ok(timestamp) = DateTime::parse_from_rfc3339(timestamp_raw) else {
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
            timestamp: timestamp.to_utc(),
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
        .find_map(|key| {
            value
                .get(*key)
                .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        })
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
        .filter(|line| line.contains("\"token_count\""))
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
        let Some(timestamp) = object.get("timestamp").and_then(Value::as_str) else {
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

fn aggregate(events: Vec<TokenEvent>, now: DateTime<Utc>, fast_tier: bool) -> UsageHistory {
    let today = now.with_timezone(&Local).date_naive();
    let yesterday = today.checked_sub_days(Days::new(1)).unwrap_or(today);
    let since = today.checked_sub_days(Days::new(29)).unwrap_or(today);
    let mut seen = HashSet::new();
    let mut days: BTreeMap<NaiveDate, DayAccumulator> = BTreeMap::new();

    for event in events {
        if !seen.insert(event.clone()) {
            continue;
        }
        let date = event.timestamp.with_timezone(&Local).date_naive();
        if date < since || date > today {
            continue;
        }
        let day = days.entry(date).or_default();
        day.tokens += event.total;
        if let Some(cost) = estimate_cost(&event, fast_tier) {
            day.cost += cost;
            day.priced_events += 1;
        } else if event.total > 0 {
            day.unpriced_events += 1;
            if !event.model.trim().is_empty() {
                day.unknown_models.insert(event.model.clone());
            }
        }
    }

    let daily = days
        .iter()
        .rev()
        .map(|(date, day)| daily_usage(*date, day))
        .collect::<Vec<_>>();
    let today_period = days.get(&today).map(usage_period);
    let yesterday_period = days.get(&yesterday).map(usage_period);
    let total = days
        .values()
        .fold(DayAccumulator::default(), |mut total, day| {
            total.tokens += day.tokens;
            total.cost += day.cost;
            total.priced_events += day.priced_events;
            total.unpriced_events += day.unpriced_events;
            total.unknown_models.extend(day.unknown_models.clone());
            total
        });
    let mut unknown_models = total.unknown_models.iter().cloned().collect::<Vec<_>>();
    unknown_models.sort();
    UsageHistory {
        today: today_period,
        yesterday: yesterday_period,
        last_30_days: (total.tokens > 0).then(|| usage_period(&total)),
        daily,
        unknown_models,
    }
}

fn daily_usage(date: NaiveDate, day: &DayAccumulator) -> DailyUsage {
    let period = usage_period(day);
    DailyUsage {
        date: date.format("%Y-%m-%d").to_string(),
        tokens: period.tokens,
        estimated_cost_usd: period.estimated_cost_usd,
        estimate_complete: period.estimate_complete,
    }
}

fn usage_period(day: &DayAccumulator) -> UsagePeriod {
    UsagePeriod {
        tokens: day.tokens,
        estimated_cost_usd: (day.priced_events > 0).then_some(day.cost),
        estimate_complete: day.unpriced_events == 0,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{aggregate, parse_jsonl};

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
    fn newly_priced_models_produce_a_complete_estimate() {
        let now = Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap();
        let content = r#"{"timestamp":"2026-07-10T08:00:00Z","type":"event_msg","payload":{"type":"token_count","model":"gpt-5.6-sol","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}}"#;
        let history = aggregate(parse_jsonl(content), now, false);
        assert_eq!(history.today.as_ref().unwrap().tokens, 110);
        assert!(history.today.as_ref().unwrap().estimated_cost_usd.is_some());
        assert!(history.today.as_ref().unwrap().estimate_complete);
        assert!(history.unknown_models.is_empty());
    }

    #[test]
    fn provider_fixture_parses_realistic_codex_jsonl() {
        let content = include_str!("../../../tests/fixtures/codex_session.jsonl");
        let events = parse_jsonl(content);
        assert_eq!(events.len(), 2);
        assert_eq!(events.iter().map(|event| event.total).sum::<u64>(), 225);
        assert!(events.iter().all(|event| event.model == "gpt-5.4"));
    }
}
