//! Read-only local log analytics. Never reads auth files or exports message content.
use crate::{
    pricing::{ModelPricing, PricingStore, TokenBreakdown},
    providers::{
        claude::local_usage as claude, codex::local_usage as codex, log_usage::load_or_parse_log,
    },
    storage::Storage,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::Arc,
};
use tauri::{AppHandle, Manager, State};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsReport {
    pub tokens: u64,
    pub estimated_cost_usd: f64,
    pub unpriced_events: usize,
    pub completed_tasks: usize,
    pub priced_tasks: usize,
    pub average_cost_usd: Option<f64>,
    pub timed_tasks: usize,
    pub average_seconds: Option<f64>,
    pub files_read: usize,
    pub unreadable_files: usize,
    pub malformed_lines: usize,
    pub earliest_event: Option<DateTime<Utc>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct ParsedLog {
    events: Vec<codex::TokenEvent>,
    claude_events: Vec<claude::ClaudeTokenEvent>,
    tasks: Vec<Task>,
    malformed: usize,
}

fn parse_log(content: &str, provider: &str) -> ParsedLog {
    let malformed = content
        .lines()
        .filter(|line| !line.trim().is_empty() && serde_json::from_str::<Value>(line).is_err())
        .count();
    if provider == "codex" {
        ParsedLog {
            events: codex::parse_jsonl(content),
            claude_events: Vec::new(),
            tasks: tasks(content),
            malformed,
        }
    } else {
        ParsedLog {
            events: Vec::new(),
            claude_events: claude::parse_jsonl(content),
            tasks: claude_tasks(content),
            malformed,
        }
    }
}

// Only explicit terminal assistant messages count as completed Claude turns.
// A subsequent user message is never used to invent a completion time.
fn claude_tasks(content: &str) -> Vec<Task> {
    let mut active: Option<(String, DateTime<Utc>)> = None;
    let mut output = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["isSidechain"] == true {
            continue;
        }
        let Some(at) = timestamp(&v["timestamp"]) else {
            continue;
        };
        if v["type"] == "user" && v["message"]["role"] == "user" && v["isMeta"] != true {
            // Tool results are not new human tasks.
            let tool_result = v["message"]["content"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "tool_result"));
            if !tool_result {
                if let Some(id) = v["uuid"].as_str() {
                    active = Some((id.to_owned(), at));
                }
            }
        } else if v["type"] == "assistant" && v["message"]["stop_reason"] == "end_turn" {
            if let Some((id, start)) = active.take().filter(|(_, start)| *start <= at) {
                output.push(Task { id, start, end: at });
            }
        }
    }
    output
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return text.parse().ok();
    }
    let seconds = value.as_f64()?;
    if !seconds.is_finite() || seconds.abs() > 253402300799.0 {
        return None;
    }
    DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64)
}

fn tasks(content: &str) -> Vec<Task> {
    let mut starts = HashMap::new();
    let mut result = Vec::new();
    let mut child_started_at = None;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["type"] == "session_meta" && codex::is_child_session_meta(&value["payload"]) {
            child_started_at = timestamp(&value["timestamp"]);
        }
        if value["type"] != "event_msg" {
            continue;
        }
        let p = &value["payload"];
        let Some(id) = p["turn_id"].as_str() else {
            continue;
        };
        let Some(at) = timestamp(&value["timestamp"]) else {
            continue;
        };
        match p["type"].as_str() {
            Some("task_started" | "turn_started") => {
                let start = timestamp(&p["started_at"]).unwrap_or(at);
                if child_started_at.is_some_and(|boundary| start < boundary) {
                    continue;
                }
                starts.insert(id.to_owned(), start);
            }
            Some("task_complete" | "turn_complete" | "turn_completed") => {
                let start = starts.remove(id).or_else(|| timestamp(&p["started_at"]));
                let end = timestamp(&p["completed_at"]).unwrap_or(at);
                if let Some(start) = start.filter(|start| *start <= end) {
                    if child_started_at.is_some_and(|boundary| start < boundary) {
                        continue;
                    }
                    result.push(Task {
                        id: id.to_owned(),
                        start,
                        end,
                    });
                }
            }
            _ => {}
        }
    }
    result
}

fn aggregate(
    logs: impl IntoIterator<Item = ParsedLog>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    pricing: &ModelPricing,
) -> AnalyticsReport {
    let mut report = AnalyticsReport::default();
    let mut seen_events = HashSet::new();
    let mut seen_tasks = HashSet::new();
    let mut task_cost = 0.0;
    let mut seconds = 0.0;
    let mut all_claude_events = Vec::new();
    for log in logs {
        report.files_read += 1;
        report.malformed_lines += log.malformed;
        let events = log.events;
        if !log.claude_events.is_empty() {
            let local_events = claude::deduplicate(log.claude_events.clone());
            for task in log.tasks {
                if task.start < start || task.end >= end || !seen_tasks.insert(task.id.clone()) {
                    continue;
                }
                report.completed_tasks += 1;
                report.timed_tasks += 1;
                seconds += (task.end - task.start).num_milliseconds() as f64 / 1000.0;
                let task_events: Vec<_> = local_events
                    .iter()
                    .filter(|e| e.timestamp >= task.start && e.timestamp <= task.end)
                    .collect();
                if task_events.is_empty() {
                    continue;
                }
                let costs: Option<Vec<f64>> = task_events
                    .iter()
                    .map(|e| claude_cost(e, pricing))
                    .collect();
                if let Some(costs) = costs {
                    report.priced_tasks += 1;
                    task_cost += costs.iter().sum::<f64>();
                }
            }
            all_claude_events.extend(log.claude_events);
            continue;
        }
        for event in &events {
            report.earliest_event = Some(
                report
                    .earliest_event
                    .map_or(event.timestamp, |earliest| earliest.min(event.timestamp)),
            );
            if event.timestamp < start || event.timestamp >= end {
                continue;
            }
            // Preserve the upstream cross-file deduplication identity.
            let key = (
                event.timestamp,
                event.model.clone(),
                event.input,
                event.cached,
                event.output,
                event.reasoning,
                event.total,
            );
            if !seen_events.insert(key) {
                continue;
            }
            report.tokens += event.total;
            match codex::estimate_cost(event, pricing) {
                Some(cost) => report.estimated_cost_usd += cost,
                None => report.unpriced_events += 1,
            }
        }
        for task in log.tasks {
            // Means use only complete turns entirely contained in the interval.
            if task.start < start || task.end >= end || !seen_tasks.insert(task.id.clone()) {
                continue;
            }
            report.completed_tasks += 1;
            report.timed_tasks += 1;
            seconds += (task.end - task.start).num_milliseconds() as f64 / 1000.0;
            let task_events: Vec<_> = events
                .iter()
                .filter(|e| e.timestamp >= task.start && e.timestamp <= task.end)
                .collect();
            if task_events.is_empty() {
                continue;
            }
            let costs: Option<Vec<f64>> = task_events
                .iter()
                .map(|e| codex::estimate_cost(e, pricing))
                .collect();
            if let Some(costs) = costs {
                report.priced_tasks += 1;
                task_cost += costs.iter().sum::<f64>();
            }
        }
    }
    let claude_events = claude::deduplicate(all_claude_events);
    for event in &claude_events {
        report.earliest_event = Some(
            report
                .earliest_event
                .map_or(event.timestamp, |earliest| earliest.min(event.timestamp)),
        );
        if event.timestamp < start || event.timestamp >= end {
            continue;
        }
        report.tokens += claude_tokens(event).total_tokens();
        match claude_cost(event, pricing) {
            Some(cost) => report.estimated_cost_usd += cost,
            None => report.unpriced_events += 1,
        }
    }
    report.average_cost_usd =
        (report.priced_tasks > 0).then(|| task_cost / report.priced_tasks as f64);
    report.average_seconds = (report.timed_tasks > 0).then(|| seconds / report.timed_tasks as f64);
    report
}

fn claude_tokens(event: &claude::ClaudeTokenEvent) -> TokenBreakdown {
    TokenBreakdown {
        input: event.input,
        cache_write_5m: event.cache_write_5m,
        cache_write_1h: event.cache_write_1h,
        cache_read: event.cache_read,
        output: event.output,
        is_fast: event.is_fast,
    }
}

fn claude_cost(event: &claude::ClaudeTokenEvent, pricing: &ModelPricing) -> Option<f64> {
    event.cost_usd.or_else(|| {
        pricing.estimated_cost_dollars(event.model.as_deref()?, claude_tokens(event), true)
    })
}

#[tauri::command]
pub async fn get_range_analytics(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    provider: String,
    pricing: State<'_, Arc<PricingStore>>,
    storage: State<'_, Arc<Storage>>,
) -> Result<AnalyticsReport, String> {
    if start >= end || end > Utc::now() + chrono::Duration::minutes(1) {
        return Err("Choose a valid past range, start inclusive and end exclusive.".into());
    }
    if !matches!(provider.as_str(), "codex" | "claude") {
        return Err("Task-level logs are unavailable for this provider.".into());
    }
    let pricing = pricing.current();
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let files = if provider == "codex" {
            let configured = crate::provider_environment::value("CODEX_HOME");
            let homes = codex::codex_homes(
                configured.as_deref().map(std::ffi::OsStr::new),
                &codex::home_directory(),
            );
            codex::discover_session_files(&homes)
        } else {
            claude::discover_files(&[], true)
        };
        let mut unreadable = 0;
        let mut cache_error = false;
        let cache_id = format!("analytics-{provider}");
        // One raw log at a time; only content-free parsed metrics enter the existing local cache.
        let logs = files.into_iter().filter_map(|path| {
            match load_or_parse_log(&storage, &cache_id, &path, 1, |content| {
                vec![parse_log(content, &provider)]
            }) {
                Ok(Some(mut logs)) => logs.pop(),
                Ok(None) => {
                    unreadable += 1;
                    None
                }
                Err(_) => {
                    cache_error = true;
                    None
                }
            }
        });
        let mut report = aggregate(logs, start, end, &pricing);
        if cache_error {
            return Err(
                "The local analytics cache is unavailable; no unreliable total was shown.".into(),
            );
        }
        report.unreadable_files = unreadable;
        Ok(report)
    })
    .await
    .map_err(|_| "Local analytics reading was interrupted.".to_owned())?
}

#[tauri::command]
pub fn get_banked_reset_marker(
    app: AppHandle,
    provider: String,
) -> Result<Option<DateTime<Utc>>, String> {
    if provider != "codex" {
        return Ok(None);
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "Local storage is unavailable.")?
        .join("analytics-reset.json");
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "The saved reset marker is unreadable.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("The reset marker could not be read.".into()),
    }
}

pub fn record_banked_reset(app: &AppHandle) -> Result<(), String> {
    use std::io::Write;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Local storage is unavailable.")?;
    fs::create_dir_all(&dir).map_err(|_| "Local storage is unavailable.")?;
    let mut file = tempfile::NamedTempFile::new_in(&dir)
        .map_err(|_| "The reset marker could not be written.")?;
    file.write_all(&serde_json::to_vec(&Utc::now()).map_err(|_| "Invalid timestamp.")?)
        .map_err(|_| "The reset marker could not be written.")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "The reset marker could not be written.")?;
    file.persist(dir.join("analytics-reset.json"))
        .map_err(|_| "The reset marker could not be written.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn aggregate(
        contents: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        pricing: &ModelPricing,
    ) -> AnalyticsReport {
        super::aggregate(
            contents.iter().map(|content| parse_log(content, "codex")),
            start,
            end,
            pricing,
        )
    }
    fn date(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }
    #[test]
    fn numeric_codex_clock_and_fork_history_are_handled() {
        let input = [
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"forked_from_id":"parent"}}"#,
            r#"{"timestamp":"2026-10-01T09:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"inherited","started_at":1790845140,"completed_at":1790845200}}"#,
            r#"{"timestamp":"2026-10-01T10:01:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"own","started_at":1790848800,"completed_at":1790848860}}"#,
        ].join("\n");
        let tasks = tasks(&input);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "own");
        assert_eq!((tasks[0].end - tasks[0].start).num_seconds(), 60);
    }
    fn claude_fixture(id: &str, cost: f64) -> String {
        [
            format!(r#"{{"timestamp":"2026-10-01T10:00:00Z","type":"user","uuid":"{id}","message":{{"role":"user","content":"synthetic prompt"}}}}"#),
            format!(r#"{{"timestamp":"2026-10-01T10:01:00Z","type":"assistant","costUSD":{cost},"message":{{"id":"{id}-message","model":"claude-opus-4-6","stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":20}}}}}}"#),
        ].join("\n")
    }
    #[test]
    fn claude_concurrent_sessions_do_not_share_costs_and_duplicates_are_ignored() {
        let a = claude_fixture("a", 1.0);
        let b = claude_fixture("b", 3.0);
        let result = super::aggregate(
            [
                parse_log(&a, "claude"),
                parse_log(&b, "claude"),
                parse_log(&a, "claude"),
            ],
            date("2026-10-01T10:00:00Z"),
            date("2026-10-01T10:02:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.tokens, 240);
        assert_eq!(result.estimated_cost_usd, 4.0);
        assert_eq!(result.completed_tasks, 2);
        assert_eq!(result.average_cost_usd, Some(2.0));
        assert_eq!(result.average_seconds, Some(60.0));
    }
    #[test]
    fn claude_requires_explicit_completion_and_cache_contains_no_prompt() {
        let input = claude_fixture("a", 1.0).replace("end_turn", "tool_use");
        let parsed = parse_log(&input, "claude");
        assert!(parsed.tasks.is_empty());
        assert!(!serde_json::to_string(&parsed)
            .unwrap()
            .contains("synthetic prompt"));
    }
    #[test]
    fn unpriced_task_is_not_a_zero_cost_task() {
        let input = fixture().replace("gpt-5", "unknown-future-model");
        let result = aggregate(
            &[input],
            date("2026-10-01T10:00:00Z"),
            date("2026-10-01T10:02:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.tokens, 120);
        assert_eq!(result.completed_tasks, 1);
        assert_eq!(result.unpriced_events, 1);
        assert_eq!(result.average_cost_usd, None);
    }
    fn fixture() -> String {
        [
            r#"{"timestamp":"2026-10-01T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"a"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:30Z","type":"turn_context","payload":{"model":"gpt-5"}}"#,
            r#"{"timestamp":"2026-10-01T10:00:40Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20,"reasoning_output_tokens":0,"total_tokens":120},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20,"reasoning_output_tokens":0,"total_tokens":120}}}}"#,
            r#"{"timestamp":"2026-10-01T10:01:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"a"}}"#,
        ].join("\n")
    }
    #[test]
    fn range_cost_and_real_completed_turn_duration() {
        let result = aggregate(
            &[fixture()],
            date("2026-10-01T10:00:00Z"),
            date("2026-10-01T10:02:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.tokens, 120);
        assert_eq!(result.completed_tasks, 1);
        assert_eq!(result.priced_tasks, 1);
        assert_eq!(result.average_seconds, Some(60.0));
        assert_eq!(result.average_cost_usd, Some(result.estimated_cost_usd));
    }
    #[test]
    fn duplicate_logs_are_not_double_counted() {
        let result = aggregate(
            &[fixture(), fixture()],
            date("2026-10-01T10:00:00Z"),
            date("2026-10-01T10:02:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.tokens, 120);
        assert_eq!(result.completed_tasks, 1);
    }
    #[test]
    fn partial_turns_excluded_from_means_not_consumption() {
        let result = aggregate(
            &[fixture()],
            date("2026-10-01T10:00:35Z"),
            date("2026-10-01T10:01:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.tokens, 120);
        assert_eq!(result.completed_tasks, 0);
        assert_eq!(result.average_seconds, None);
        assert_eq!(result.average_cost_usd, None);
    }
    #[test]
    fn incomplete_turn_and_malformed_line_do_not_create_task() {
        let input = fixture().lines().take(3).collect::<Vec<_>>().join("\n") + "\n{bad";
        let result = aggregate(
            &[input],
            date("2026-10-01T10:00:00Z"),
            date("2026-10-01T10:02:00Z"),
            &crate::pricing::test_bundled_pricing(),
        );
        assert_eq!(result.completed_tasks, 0);
        assert_eq!(result.malformed_lines, 1);
    }
}
