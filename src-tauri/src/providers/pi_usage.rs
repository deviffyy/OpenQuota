use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{
    pricing::{ModelPricing, TokenBreakdown},
    storage::Storage,
};

use super::{
    daily_usage::DailyUsageAccumulator,
    log_usage::{load_or_parse_log, parse_log_timestamp, LogCacheError},
};

const LOG_CACHE_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PiUsageEvent {
    id: Option<String>,
    timestamp: DateTime<Utc>,
    card_id: String,
    model: String,
    carried_cost: Option<f64>,
    tokens: PiTokenBreakdown,
    reported_total_tokens: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct PiTokenBreakdown {
    input: u64,
    cache_write_5m: u64,
    cache_write_1h: u64,
    cache_read: u64,
    output: u64,
}

impl PiTokenBreakdown {
    fn pricing_tokens(self) -> TokenBreakdown {
        TokenBreakdown {
            input: self.input,
            cache_write_5m: self.cache_write_5m,
            cache_write_1h: self.cache_write_1h,
            cache_read: self.cache_read,
            output: self.output,
            is_fast: false,
        }
    }
}

/// Folds usage written by pi into the card of the underlying model provider. Returns whether at
/// least one event contributed usage or an unknown-model marker to that card.
pub fn scan_into(
    storage: &Storage,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    card_id: &str,
    accumulator: &mut DailyUsageAccumulator,
) -> Result<bool, LogCacheError> {
    let home = home_directory();
    let directory = sessions_directory(
        env_text("PI_CODING_AGENT_SESSION_DIR").as_deref(),
        env_text("PI_CODING_AGENT_DIR").as_deref(),
        &home,
    );
    scan_directory_into(storage, &directory, now, pricing, card_id, accumulator)
}

fn scan_directory_into(
    storage: &Storage,
    directory: &Path,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    card_id: &str,
    accumulator: &mut DailyUsageAccumulator,
) -> Result<bool, LogCacheError> {
    let paths = discover_files(directory);
    let mut seen_paths = HashSet::with_capacity(paths.len());
    let mut events = Vec::new();
    for path in paths {
        seen_paths.insert(path.clone());
        let Some(parsed) =
            load_or_parse_log(storage, "pi", &path, LOG_CACHE_SCHEMA_VERSION, parse_jsonl)?
        else {
            continue;
        };
        events.extend(parsed);
    }
    storage.prune_log_events("pi", &seen_paths)?;

    let since = now
        .with_timezone(&Local)
        .date_naive()
        .checked_sub_days(Days::new(30))
        .unwrap_or(NaiveDate::MIN);
    Ok(aggregate_into(
        deduplicate(events),
        card_id,
        since,
        now,
        pricing,
        accumulator,
    ))
}

fn sessions_directory(
    session_override: Option<&str>,
    config_override: Option<&str>,
    home: &Path,
) -> PathBuf {
    if let Some(path) = session_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return expand_home(path, home);
    }
    if let Some(path) = config_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return expand_home(path, home).join("sessions");
    }
    home.join(".pi").join("agent").join("sessions")
}

fn discover_files(directory: &Path) -> Vec<PathBuf> {
    let directory = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let mut paths = WalkDir::new(directory)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl")
        })
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn parse_jsonl(content: &str) -> Vec<PiUsageEvent> {
    content
        .lines()
        .filter(|line| line.contains("\"usage\""))
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<PiUsageEvent> {
    let object = serde_json::from_str::<Value>(line).ok()?;
    if object.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let timestamp = parse_log_timestamp(object.get("timestamp")?.as_str()?)?;
    let message = object.get("message")?.as_object()?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let card_id = mapped_card(message.get("provider")?.as_str()?)?;
    let usage = message.get("usage")?.as_object()?;
    let cache_write = unsigned_number(usage.get("cacheWrite")).unwrap_or_default();
    let cache_write_1h = unsigned_number(usage.get("cacheWrite1h")).unwrap_or_default();
    let carried_cost = usage
        .get("cost")
        .and_then(Value::as_object)
        .and_then(|cost| finite_number(cost.get("total")));

    Some(PiUsageEvent {
        id: object.get("id").and_then(Value::as_str).map(str::to_owned),
        timestamp,
        card_id: card_id.to_owned(),
        model: message
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned(),
        carried_cost,
        tokens: PiTokenBreakdown {
            input: unsigned_number(usage.get("input")).unwrap_or_default(),
            cache_write_5m: cache_write.saturating_sub(cache_write_1h),
            cache_write_1h,
            cache_read: unsigned_number(usage.get("cacheRead")).unwrap_or_default(),
            output: unsigned_number(usage.get("output")).unwrap_or_default(),
        },
        reported_total_tokens: unsigned_number(usage.get("totalTokens")).unwrap_or_default(),
    })
}

fn mapped_card(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" | "claude-agent-sdk" => Some("claude"),
        "openai-codex" => Some("codex"),
        "cursor" => Some("cursor"),
        "zai" | "zhipu" => Some("zai"),
        "google-antigravity" => Some("antigravity"),
        "github-copilot" => Some("copilot"),
        _ => None,
    }
}

fn deduplicate(events: Vec<PiUsageEvent>) -> Vec<PiUsageEvent> {
    let mut seen = HashSet::new();
    events
        .into_iter()
        .filter(|event| event.id.as_ref().is_none_or(|id| seen.insert(id.clone())))
        .collect()
}

fn aggregate_into(
    events: Vec<PiUsageEvent>,
    card_id: &str,
    since: NaiveDate,
    now: DateTime<Utc>,
    pricing: &ModelPricing,
    accumulator: &mut DailyUsageAccumulator,
) -> bool {
    let mut contributed = false;
    for event in events {
        if event.card_id != card_id || event.timestamp > now {
            continue;
        }
        let date = event.timestamp.with_timezone(&Local).date_naive();
        if date < since {
            continue;
        }
        let model = event.model.trim();
        let display_model = if model.is_empty() {
            "Unattributed"
        } else {
            model
        };
        if let Some(cost) = event.carried_cost.filter(|cost| *cost > 0.0) {
            accumulator.add_exact(date, event.reported_total_tokens, cost, display_model);
            contributed = true;
        } else if !model.is_empty() {
            if let Some(cost) =
                pricing.estimated_cost_dollars(model, event.tokens.pricing_tokens(), true)
            {
                accumulator.add(date, event.reported_total_tokens, cost, model);
                contributed = true;
            } else if event.reported_total_tokens > 0 {
                accumulator.add_unknown_model(date, model);
                contributed = true;
            }
        }
    }
    contributed
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    let number = match value? {
        Value::Number(value) => value.as_f64()?,
        Value::String(value) => value.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

fn unsigned_number(value: Option<&Value>) -> Option<u64> {
    let number = finite_number(value)?;
    if number < 0.0 || number > u64::MAX as f64 {
        return None;
    }
    Some(number.trunc() as u64)
}

fn env_text(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, path::Path};

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::{
        aggregate_into, deduplicate, mapped_card, parse_line, scan_directory_into,
        sessions_directory,
    };
    use crate::{
        pricing::{ModelPricing, ModelRates, PricingCatalog, PricingSupplement},
        providers::daily_usage::DailyUsageAccumulator,
        storage::Storage,
    };

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
    }

    fn line(id: &str, provider: &str, model: &str, cost: &str) -> String {
        let cost = serde_json::from_str::<serde_json::Value>(cost).unwrap();
        serde_json::json!({
            "type": "message",
            "id": id,
            "timestamp": "2026-07-12T10:00:00.000Z",
            "message": {
                "role": "assistant",
                "provider": provider,
                "model": model,
                "usage": {
                    "input": 100,
                    "output": 50,
                    "cacheRead": 10,
                    "cacheWrite": 30,
                    "cacheWrite1h": 12,
                    "totalTokens": 202,
                    "cost": {"total": cost}
                }
            }
        })
        .to_string()
    }

    fn pricing() -> ModelPricing {
        ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("priced-model".into(), ModelRates::new(10.0, 20.0))]),
                ..PricingCatalog::default()
            },
            PricingCatalog::default(),
        )
    }

    #[test]
    fn path_resolution_uses_session_then_config_then_default() {
        let home = Path::new("/home/tester");
        assert_eq!(
            sessions_directory(Some("~/pi-sessions"), Some("~/ignored"), home),
            home.join("pi-sessions")
        );
        assert_eq!(
            sessions_directory(None, Some("~/pi-config"), home),
            home.join("pi-config").join("sessions")
        );
        assert_eq!(
            sessions_directory(Some("  "), Some("  "), home),
            home.join(".pi").join("agent").join("sessions")
        );
    }

    #[test]
    fn parser_maps_cards_splits_cache_and_accepts_numeric_strings() {
        let event = parse_line(&line("one", "anthropic", "claude-model", "\"0.5\"")).unwrap();
        assert_eq!(event.card_id, "claude");
        assert_eq!(event.carried_cost, Some(0.5));
        assert_eq!(event.tokens.cache_write_5m, 18);
        assert_eq!(event.tokens.cache_write_1h, 12);
        assert_eq!(event.reported_total_tokens, 202);
        assert_eq!(mapped_card("openai-codex"), Some("codex"));
        assert_eq!(mapped_card("nvidia-nim"), None);
    }

    #[test]
    fn parser_rejects_unmapped_and_non_assistant_messages() {
        assert!(parse_line(&line("one", "nvidia-nim", "model", "1")).is_none());
        let user = r#"{"type":"message","timestamp":"2026-07-12T10:00:00Z","message":{"role":"user","provider":"anthropic","usage":{}}}"#;
        assert!(parse_line(user).is_none());
    }

    #[test]
    fn carried_cost_is_exact_zero_cost_is_priced_and_unknowns_are_reported() {
        let events = vec![
            parse_line(&line("exact", "anthropic", "unknown-exact", "0.5")).unwrap(),
            parse_line(&line("priced", "anthropic", "priced-model", "0")).unwrap(),
            parse_line(&line("unknown", "anthropic", "missing-model", "0")).unwrap(),
        ];
        let mut accumulator = DailyUsageAccumulator::default();
        assert!(aggregate_into(
            events,
            "claude",
            chrono::NaiveDate::MIN,
            now(),
            &pricing(),
            &mut accumulator,
        ));
        let history = accumulator.build(now(), "From pi");
        let period = history.today.unwrap();
        assert_eq!(period.tokens, 404);
        assert!(period.cost_estimated);
        assert!((period.estimated_cost_usd.unwrap() - 0.502_43).abs() < 0.000_001);
        assert_eq!(period.unknown_models, ["missing-model"]);
    }

    #[test]
    fn future_dated_events_do_not_contribute_usage() {
        let mut event = parse_line(&line("future", "anthropic", "priced-model", "0.5")).unwrap();
        event.timestamp = now() + chrono::Duration::seconds(1);
        let mut accumulator = DailyUsageAccumulator::default();

        assert!(!aggregate_into(
            vec![event],
            "claude",
            chrono::NaiveDate::MIN,
            now(),
            &pricing(),
            &mut accumulator,
        ));
        assert!(accumulator.build(now(), "From pi").today.is_none());
    }

    #[test]
    fn repeated_message_ids_are_counted_once_across_files() {
        let event = parse_line(&line("duplicate", "anthropic", "model", "0.5")).unwrap();
        assert_eq!(deduplicate(vec![event.clone(), event]).len(), 1);
    }

    #[test]
    fn recursive_scan_uses_cache_and_folds_only_the_requested_card() {
        let directory = tempdir().unwrap();
        let sessions = directory.path().join("sessions");
        let log = sessions.join("project").join("session.jsonl");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(
            &log,
            [
                line("claude", "anthropic", "claude-model", "0.5"),
                line("codex", "openai-codex", "gpt-model", "0.25"),
            ]
            .join("\n"),
        )
        .unwrap();
        let storage = Storage::open(&directory.path().join("cache.db")).unwrap();
        let mut accumulator = DailyUsageAccumulator::default();

        assert!(scan_directory_into(
            &storage,
            &sessions,
            now(),
            &pricing(),
            "claude",
            &mut accumulator,
        )
        .unwrap());
        let history = accumulator.build(now(), "From pi");
        assert_eq!(history.today.unwrap().tokens, 202);

        let mut second = DailyUsageAccumulator::default();
        assert!(
            scan_directory_into(&storage, &sessions, now(), &pricing(), "codex", &mut second,)
                .unwrap()
        );
        assert_eq!(second.build(now(), "From pi").today.unwrap().tokens, 202);
    }
}
