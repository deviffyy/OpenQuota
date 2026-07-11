use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::PathBuf,
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

use super::{auth::claude_home, ClaudeError};

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
    pub cost_usd: Option<f64>,
}

impl ClaudeTokenEvent {
    fn total_tokens(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }
}

#[derive(Default)]
struct DayAccumulator {
    tokens: u64,
    cost: f64,
    complete: bool,
    unknown_models: HashSet<String>,
}

pub fn scan_local_usage(
    storage: &Storage,
    now: DateTime<Utc>,
) -> Result<UsageHistory, ClaudeError> {
    let since_date = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(29))
        .unwrap_or(NaiveDate::MIN);
    let mut events = Vec::new();
    for path in discover_files() {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_millis() as i64)
            .unwrap_or_default();
        let cached = storage
            .load_log_events(&path, metadata.len(), modified_millis)
            .map_err(|_| ClaudeError::LocalUsage)?;
        let parsed = if let Some(parsed) = cached
            .as_deref()
            .and_then(|json| serde_json::from_str::<Vec<ClaudeTokenEvent>>(json).ok())
        {
            parsed
        } else {
            let parsed = fs::read_to_string(&path)
                .map(|content| parse_jsonl(&content))
                .unwrap_or_default();
            let json = serde_json::to_string(&parsed).map_err(|_| ClaudeError::LocalUsage)?;
            storage
                .save_log_events(&path, metadata.len(), modified_millis, &json)
                .map_err(|_| ClaudeError::LocalUsage)?;
            parsed
        };
        events.extend(
            parsed
                .into_iter()
                .filter(|event| event.timestamp.with_timezone(&Local).date_naive() >= since_date),
        );
    }
    Ok(aggregate(deduplicate(events), now))
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
                    && event.total_tokens() > existing.total_tokens());
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

fn aggregate(events: Vec<ClaudeTokenEvent>, now: DateTime<Utc>) -> UsageHistory {
    let today = now.with_timezone(&Local).date_naive();
    let start = today.checked_sub_days(Days::new(29)).unwrap_or(today);
    let mut days = BTreeMap::<NaiveDate, DayAccumulator>::new();
    for offset in 0..30 {
        if let Some(date) = start.checked_add_days(Days::new(offset)) {
            days.insert(
                date,
                DayAccumulator {
                    complete: true,
                    ..DayAccumulator::default()
                },
            );
        }
    }
    for event in events {
        let date = event.timestamp.with_timezone(&Local).date_naive();
        let Some(day) = days.get_mut(&date) else {
            continue;
        };
        day.tokens += event.total_tokens();
        if let Some(cost) = event.cost_usd.or_else(|| estimate_cost(&event)) {
            day.cost += cost;
        } else if event.total_tokens() > 0 {
            day.complete = false;
            if let Some(model) = event.model {
                day.unknown_models.insert(model);
            }
        }
    }
    let daily = days
        .iter()
        .map(|(date, day)| DailyUsage {
            date: date.to_string(),
            tokens: day.tokens,
            estimated_cost_usd: (day.complete || day.cost > 0.0).then_some(day.cost),
            estimate_complete: day.complete,
        })
        .collect::<Vec<_>>();
    let yesterday = today.checked_sub_days(Days::new(1));
    let period_for = |date: Option<NaiveDate>| {
        let day = days.get(&date?)?;
        Some(UsagePeriod {
            tokens: day.tokens,
            estimated_cost_usd: (day.complete || day.cost > 0.0).then_some(day.cost),
            estimate_complete: day.complete,
        })
    };
    let mut unknown_models = days
        .values()
        .flat_map(|day| day.unknown_models.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    unknown_models.sort();
    UsageHistory {
        today: period_for(Some(today)),
        yesterday: period_for(yesterday),
        last_30_days: Some(UsagePeriod {
            tokens: days.values().map(|day| day.tokens).sum(),
            estimated_cost_usd: days
                .values()
                .all(|day| day.complete)
                .then(|| days.values().map(|day| day.cost).sum()),
            estimate_complete: days.values().all(|day| day.complete),
        }),
        daily,
        unknown_models,
    }
}

fn estimate_cost(event: &ClaudeTokenEvent) -> Option<f64> {
    let model = event.model.as_deref()?.to_ascii_lowercase();
    let (input, output) = if model.contains("fable-5") || model.contains("mythos-5") {
        (10.0, 50.0)
    } else if model.contains("opus-4-8")
        || model.contains("opus-4-7")
        || model.contains("opus-4-6")
        || model.contains("opus-4-5")
    {
        (5.0, 25.0)
    } else if model.contains("opus-4") || model.contains("opus-3") {
        (15.0, 75.0)
    } else if model.contains("sonnet-5") {
        (2.0, 10.0)
    } else if model.contains("sonnet-4") || model.contains("sonnet-3") {
        (3.0, 15.0)
    } else if model.contains("haiku-4-5") {
        (1.0, 5.0)
    } else if model.contains("haiku-3-5") {
        (0.8, 4.0)
    } else if model.contains("haiku-3") {
        (0.25, 1.25)
    } else {
        return None;
    };
    Some(
        (event.input as f64 * input
            + event.cache_write_5m as f64 * input * 1.25
            + event.cache_write_1h as f64 * input * 2.0
            + event.cache_read as f64 * input * 0.1
            + event.output as f64 * output)
            / 1_000_000.0,
    )
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
    use super::{deduplicate, estimate_cost, parse_jsonl};

    #[test]
    fn provider_fixture_parses_and_deduplicates_claude_usage_lines() {
        let events = parse_jsonl(include_str!("fixtures/usage.jsonl"));
        assert_eq!(events.len(), 2);
        let events = deduplicate(events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].total_tokens(), 170);
        assert!(estimate_cost(&events[0]).unwrap() > 0.0);
    }
}
