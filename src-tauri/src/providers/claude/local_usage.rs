use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::UNIX_EPOCH,
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

use super::{auth::claude_home, ClaudeError};
use crate::providers::daily_usage::DailyUsageAccumulator;

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

const LOG_CACHE_SCHEMA_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
struct CachedClaudeEvents {
    schema_version: u8,
    events: Vec<ClaudeTokenEvent>,
}

impl ClaudeTokenEvent {
    fn total_tokens(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }
}

pub fn scan_local_usage(
    storage: &Storage,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
) -> Result<UsageHistory, ClaudeError> {
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    let mut events = Vec::new();
    let paths = discover_files();
    let mut seen_paths = HashSet::with_capacity(paths.len());
    for path in paths {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        seen_paths.insert(path.clone());
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_millis() as i64)
            .unwrap_or_default();
        let cached = storage
            .load_log_events("claude", &path, metadata.len(), modified_millis)
            .map_err(|_| ClaudeError::LocalUsage)?;
        let parsed = if let Some(parsed) = cached.as_deref().and_then(|json| {
            serde_json::from_str::<CachedClaudeEvents>(json)
                .ok()
                .filter(|cache| cache.schema_version == LOG_CACHE_SCHEMA_VERSION)
                .map(|cache| cache.events)
        }) {
            parsed
        } else {
            let parsed = fs::read_to_string(&path)
                .map(|content| parse_jsonl(&content))
                .unwrap_or_default();
            let json = serde_json::to_string(&CachedClaudeEvents {
                schema_version: LOG_CACHE_SCHEMA_VERSION,
                events: parsed.clone(),
            })
            .map_err(|_| ClaudeError::LocalUsage)?;
            storage
                .save_log_events("claude", &path, metadata.len(), modified_millis, &json)
                .map_err(|_| ClaudeError::LocalUsage)?;
            parsed
        };
        events.extend(
            parsed
                .into_iter()
                .filter(|event| event.timestamp.with_timezone(&Local).date_naive() >= since_date),
        );
    }
    storage
        .prune_log_events("claude", &seen_paths)
        .map_err(|_| ClaudeError::LocalUsage)?;
    Ok(aggregate(deduplicate(events), now, pricing))
}

fn discover_files() -> Vec<PathBuf> {
    let projects = claude_home().join("projects");
    if !projects.is_dir() {
        return Vec::new();
    }
    WalkDir::new(projects)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl")
        })
        .map(|entry| entry.into_path())
        .collect()
}

pub fn parse_jsonl(content: &str) -> Vec<ClaudeTokenEvent> {
    content
        .lines()
        .filter(|line| line.contains("\"usage\""))
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<ClaudeTokenEvent> {
    let object: Value = serde_json::from_str(line).ok()?;
    let timestamp = DateTime::parse_from_rfc3339(object.get("timestamp")?.as_str()?)
        .ok()?
        .to_utc();
    let message = object.get("message")?;
    let usage = message.get("usage")?;
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
        model: message
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "<synthetic>")
            .map(str::to_owned),
        input,
        cache_write_5m,
        cache_write_1h,
        cache_read: integer(usage.get("cache_read_input_tokens")).unwrap_or_default(),
        output,
        message_id: message.get("id").and_then(Value::as_str).map(str::to_owned),
        request_id: object
            .get("requestId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        sidechain: object
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_fast: speed == Some("fast"),
        has_speed: speed.is_some(),
        cost_usd: object.get("costUSD").and_then(number),
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

fn aggregate(
    events: Vec<ClaudeTokenEvent>,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
) -> UsageHistory {
    let since = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    let mut accumulator = DailyUsageAccumulator::default();
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
    accumulator.build(now, "From your Claude usage history (estimated)")
}

fn integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64().or_else(|| value?.as_str()?.parse().ok())
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{
        aggregate, deduplicate, parse_jsonl, CachedClaudeEvents, LOG_CACHE_SCHEMA_VERSION,
    };
    use crate::pricing::test_bundled_pricing;

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

    #[test]
    fn old_log_cache_shape_is_rejected_after_speed_support() {
        let events = parse_jsonl(
            r#"{"timestamp":"2026-07-15T08:00:00Z","message":{"model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":10,"speed":"fast"}}}"#,
        );
        let old_cache = serde_json::to_string(&events).unwrap();
        assert!(serde_json::from_str::<CachedClaudeEvents>(&old_cache).is_err());

        let current = serde_json::to_string(&CachedClaudeEvents {
            schema_version: LOG_CACHE_SCHEMA_VERSION,
            events,
        })
        .unwrap();
        let decoded = serde_json::from_str::<CachedClaudeEvents>(&current).unwrap();
        assert_eq!(decoded.schema_version, LOG_CACHE_SCHEMA_VERSION);
        assert!(decoded.events[0].is_fast);
    }
}
