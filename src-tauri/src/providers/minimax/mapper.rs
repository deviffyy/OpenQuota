use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

use super::MiniMaxError;

const WEEKLY_PERIOD_SECONDS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_INTERVAL_PERIOD_SECONDS: u64 = 5 * 60 * 60;

#[derive(Debug, PartialEq)]
pub struct MiniMaxMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
}

/// Returns the API-level error message when `base_resp.status_code` is non-zero.
pub fn api_error_message(body: &Value) -> Option<String> {
    let code = number(body.get("base_resp")?.get("status_code"))?;
    if code == 0.0 {
        None
    } else {
        Some(
            body.get("base_resp")
                .and_then(|base| base.get("status_msg"))
                .and_then(Value::as_str)
                .unwrap_or("MiniMax error")
                .to_owned(),
        )
    }
}

pub fn map_usage(body: &Value) -> Result<MiniMaxMappedUsage, MiniMaxError> {
    if let Some(message) = api_error_message(body) {
        let normalized = message.to_ascii_lowercase();
        if normalized.contains("no plan") || normalized.contains("token plan") || normalized.contains("subscribe")
        {
            return Err(MiniMaxError::NoTokenPlan);
        }
        return Err(MiniMaxError::InvalidResponse);
    }

    let models = body
        .get("model_remains")
        .and_then(Value::as_array)
        .ok_or(MiniMaxError::InvalidResponse)?;
    let general = models
        .iter()
        .find(|model| model.get("model_name").and_then(Value::as_str) == Some("general"))
        .or_else(|| models.first())
        .ok_or(MiniMaxError::InvalidResponse)?;

    let weekly = quota_from_model(general, Window::Weekly)?;
    let session = quota_from_model(general, Window::Interval)?;
    Ok(MiniMaxMappedUsage {
        plan: Some("Token Plan".into()),
        quotas: vec![session, weekly],
    })
}

#[derive(Clone, Copy)]
enum Window {
    Weekly,
    Interval,
}

fn quota_from_model(model: &Value, window: Window) -> Result<QuotaWindow, MiniMaxError> {
    let (remaining_key, end_key, start_key, id, label, default_period) = match window {
        Window::Weekly => (
            "current_weekly_remaining_percent",
            "weekly_end_time",
            "weekly_start_time",
            "weekly",
            "Weekly",
            WEEKLY_PERIOD_SECONDS,
        ),
        Window::Interval => (
            "current_interval_remaining_percent",
            "end_time",
            "start_time",
            "session",
            "Session",
            DEFAULT_INTERVAL_PERIOD_SECONDS,
        ),
    };
    let remaining = number(model.get(remaining_key))
        .filter(|value| (0.0..=100.0).contains(value))
        .ok_or(MiniMaxError::InvalidResponse)?;
    let used_percent = (100.0 - remaining).clamp(0.0, 100.0);

    let end = number(model.get(end_key));
    let start = number(model.get(start_key));
    let period_seconds = match (start, end) {
        (Some(start), Some(end)) if end > start => ((end - start) / 1000.0) as u64,
        _ => default_period,
    };
    let resets_at = end.and_then(millis_time);

    Ok(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent,
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    })
}

fn millis_time(milliseconds: f64) -> Option<DateTime<Utc>> {
    if milliseconds < i64::MIN as f64 || milliseconds > i64::MAX as f64 {
        return None;
    }
    DateTime::from_timestamp_millis(milliseconds.trunc() as i64)
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        })
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};

    use super::{api_error_message, map_usage};
    use crate::providers::minimax::MiniMaxError;

    fn captured() -> Value {
        serde_json::from_str(
            r#"{
            "model_remains":[{
                "start_time":1786060800000,"end_time":1786078800000,
                "remains_time":2185461,
                "current_interval_total_count":0,"current_interval_usage_count":0,
                "model_name":"general",
                "current_weekly_total_count":0,"current_weekly_usage_count":0,
                "weekly_start_time":1785715200000,"weekly_end_time":1786320000000,
                "weekly_remains_time":243385461,
                "current_interval_status":2,"current_interval_remaining_percent":0,
                "current_weekly_status":3,"current_weekly_remaining_percent":100
            }],
            "base_resp":{"status_code":0,"status_msg":"success"}
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn captured_payload_maps_session_and_weekly_for_general() {
        let mapped = map_usage(&captured()).unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Token Plan"));
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly"]
        );

        let session = &mapped.quotas[0];
        assert_eq!(session.used_percent, 100.0);
        assert_eq!(
            session.resets_at,
            Utc.timestamp_millis_opt(1_786_078_800_000).single()
        );
        assert_eq!(session.period_seconds, 5 * 60 * 60);

        let weekly = &mapped.quotas[1];
        assert_eq!(weekly.used_percent, 0.0);
        assert_eq!(
            weekly.resets_at,
            Utc.timestamp_millis_opt(1_786_320_000_000).single()
        );
        // weekly window: 1786320000000 - 1785715200000 = 604_800_000 ms = 7 days
        assert_eq!(weekly.period_seconds, 7 * 24 * 60 * 60);
    }

    #[test]
    fn falls_back_to_first_model_when_general_is_absent() {
        let mapped = map_usage(&json!({
            "model_remains":[{
                "model_name":"video","current_weekly_remaining_percent":40,
                "current_interval_remaining_percent":90,
                "weekly_start_time":0,"weekly_end_time":604800000,
                "start_time":0,"end_time":18000000
            }],
            "base_resp":{"status_code":0}
        }))
        .unwrap();
        let weekly = mapped.quotas.iter().find(|q| q.id == "weekly").unwrap();
        assert_eq!(weekly.used_percent, 60.0);
    }

    #[test]
    fn api_error_messages_classify_no_plan_and_generic_failures() {
        assert_eq!(
            api_error_message(&json!({"base_resp":{"status_code":0}})),
            None
        );
        assert_eq!(
            api_error_message(&json!({"base_resp":{"status_code":1001,"status_msg":"no token plan"}}))
                .as_deref(),
            Some("no token plan")
        );

        assert!(matches!(
            map_usage(&json!({"base_resp":{"status_code":1001,"status_msg":"user has no token plan"}})),
            Err(MiniMaxError::NoTokenPlan)
        ));
        assert!(matches!(
            map_usage(&json!({"base_resp":{"status_code":500,"status_msg":"internal error"}})),
            Err(MiniMaxError::InvalidResponse)
        ));
        assert!(map_usage(&json!({"base_resp":{"status_code":0}})).is_err());
    }
}
