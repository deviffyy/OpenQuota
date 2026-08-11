use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

use super::KimiError;

const WEEKLY_PERIOD_SECONDS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_WINDOW_PERIOD_SECONDS: u64 = 5 * 60 * 60;

#[derive(Debug, PartialEq)]
pub struct KimiMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
}

pub fn map_usage(body: &Value) -> Result<KimiMappedUsage, KimiError> {
    Ok(KimiMappedUsage {
        plan: plan_name(body),
        quotas: map_quotas(body)?,
    })
}

fn plan_name(body: &Value) -> Option<String> {
    let raw = body
        .get("user")?
        .get("membership")?
        .get("level")?
        .as_str()?;
    let stripped = raw.trim().strip_prefix("LEVEL_").unwrap_or(raw).trim();
    if stripped.is_empty() {
        None
    } else {
        Some(title_case(stripped))
    }
}

fn title_case(value: &str) -> String {
    let mut out = String::new();
    let mut new_word = true;
    for ch in value.chars() {
        if ch == '_' {
            new_word = true;
            out.push(' ');
            continue;
        }
        if new_word {
            out.extend(ch.to_uppercase());
            new_word = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

fn map_quotas(body: &Value) -> Result<Vec<QuotaWindow>, KimiError> {
    // Convention: the short rolling window is the Session quota and the main usage quota is the
    // Weekly quota. Session is shown first.
    let mut quotas = Vec::new();
    if let Ok(Some(session)) = session_quota(body) {
        quotas.push(session);
    }
    if let Ok(weekly) = weekly_quota(body) {
        quotas.push(weekly);
    }
    (!quotas.is_empty())
        .then_some(quotas)
        .ok_or(KimiError::InvalidResponse)
}

fn weekly_quota(body: &Value) -> Result<QuotaWindow, KimiError> {
    let usage = body
        .get("usage")
        .and_then(Value::as_object)
        .ok_or(KimiError::InvalidResponse)?;
    let limit = number(usage.get("limit"))
        .filter(|value| *value >= 0.0)
        .ok_or(KimiError::InvalidResponse)?;
    let used = number(usage.get("used"))
        .filter(|value| *value >= 0.0)
        .ok_or(KimiError::InvalidResponse)?;
    let used_percent = if limit > 0.0 {
        (used / limit * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    Ok(QuotaWindow {
        id: "weekly".into(),
        label: "Weekly".into(),
        used_percent,
        resets_at: iso_time(usage.get("resetTime")),
        period_seconds: WEEKLY_PERIOD_SECONDS,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    })
}

fn session_quota(body: &Value) -> Result<Option<QuotaWindow>, KimiError> {
    let Some(entry) = body
        .get("limits")
        .and_then(Value::as_array)
        .and_then(|limits| {
            limits
                .iter()
                .find(|entry| window_period_seconds(entry) == Some(DEFAULT_WINDOW_PERIOD_SECONDS))
        })
    else {
        return Ok(None);
    };
    let detail = entry
        .get("detail")
        .and_then(Value::as_object)
        .ok_or(KimiError::InvalidResponse)?;
    let limit = number(detail.get("limit"))
        .filter(|value| *value >= 0.0)
        .ok_or(KimiError::InvalidResponse)?;
    let remaining = number(detail.get("remaining"))
        .filter(|value| *value >= 0.0)
        .ok_or(KimiError::InvalidResponse)?;
    let used = (limit - remaining).max(0.0);
    let used_percent = if limit > 0.0 {
        (used / limit * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let period_seconds = window_period_seconds(entry).unwrap_or(DEFAULT_WINDOW_PERIOD_SECONDS);

    Ok(Some(QuotaWindow {
        id: "session".into(),
        label: "Session".into(),
        used_percent,
        resets_at: iso_time(detail.get("resetTime")),
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    }))
}

fn window_period_seconds(entry: &Value) -> Option<u64> {
    let window = entry.get("window")?;
    let duration = number(window.get("duration")).filter(|value| *value > 0.0)?;
    let factor = match window.get("timeUnit").and_then(Value::as_str) {
        Some("TIME_UNIT_HOUR") => 3600.0,
        Some("TIME_UNIT_SECOND") => 1.0,
        Some("TIME_UNIT_MINUTE") => 60.0,
        _ => return None,
    };
    Some((duration * factor) as u64)
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

fn iso_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = value?.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|datetime| datetime.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};

    use super::{map_usage, plan_name, DEFAULT_WINDOW_PERIOD_SECONDS};
    use crate::models::QuotaFormat;

    fn captured() -> Value {
        serde_json::json!({
            "user": {"membership":{"level":"LEVEL_BASIC"}},
            "usage": {"limit":"100","used":"25","resetTime":"2026-08-10T02:17:43.139020Z"},
            "limits": [{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                        "detail":{"limit":"100","remaining":"80",
                                  "resetTime":"2026-08-07T06:17:43.139020Z"}}],
            "parallel": {"limit":"10"}
        })
    }

    #[test]
    fn captured_payload_maps_usage_window_and_plan() {
        let mapped = map_usage(&captured()).unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Basic"));
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly"]
        );

        let session = &mapped.quotas[0];
        assert_eq!(session.used_percent, 20.0);
        assert_eq!(session.period_seconds, 5 * 60 * 60);

        let weekly = &mapped.quotas[1];
        assert_eq!(weekly.used_percent, 25.0);
        assert_eq!(weekly.format, QuotaFormat::Percent);
        assert_eq!(weekly.used_value, None);
        assert_eq!(weekly.limit_value, None);
        assert_eq!(weekly.unit, None);
        assert_eq!(weekly.period_seconds, 7 * 24 * 60 * 60);
        assert_eq!(
            weekly.resets_at.map(|datetime| datetime.timestamp()),
            Some(
                Utc.with_ymd_and_hms(2026, 8, 10, 2, 17, 43)
                    .single()
                    .unwrap()
                    .timestamp()
            )
        );
    }

    #[test]
    fn missing_session_is_optional_but_weekly_is_required() {
        let mapped = map_usage(&json!({
            "user": {"membership": {"level": "LEVEL_PRO"}},
            "usage": {"limit": "50", "used": "0", "resetTime": null}
        }))
        .unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Pro"));
        assert_eq!(mapped.quotas.len(), 1);
        assert_eq!(mapped.quotas[0].id, "weekly");
        assert_eq!(mapped.quotas[0].used_percent, 0.0);
        assert_eq!(mapped.quotas[0].resets_at, None);

        assert!(map_usage(&json!({"limits":[]})).is_err());
    }

    #[test]
    fn session_quota_selects_the_five_hour_window_regardless_of_order() {
        let mapped = map_usage(&json!({
            "user": {"membership": {"level": "LEVEL_PRO"}},
            "usage": {"limit": "100", "used": "0"},
            "limits": [
                {"window": {"duration": 1, "timeUnit": "TIME_UNIT_HOUR"},
                 "detail": {"limit": "100", "remaining": "0"}},
                {"window": {"duration": 5, "timeUnit": "TIME_UNIT_HOUR"},
                 "detail": {"limit": "100", "remaining": "75"}}
            ]
        }))
        .unwrap();
        let session = mapped
            .quotas
            .iter()
            .find(|quota| quota.id == "session")
            .unwrap();
        assert_eq!(session.used_percent, 25.0);
        assert_eq!(session.period_seconds, DEFAULT_WINDOW_PERIOD_SECONDS);
    }

    #[test]
    fn an_invalid_weekly_section_keeps_a_valid_session_quota() {
        let mut body = captured();
        body["usage"]["used"] = json!("not-a-number");

        let mapped = map_usage(&body).unwrap();
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session"]
        );
    }

    #[test]
    fn an_invalid_session_section_keeps_a_valid_weekly_quota() {
        let mut body = captured();
        body["limits"][0]["detail"]["remaining"] = json!("not-a-number");

        let mapped = map_usage(&body).unwrap();
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["weekly"]
        );
    }

    #[test]
    fn plan_level_is_optional_and_title_cased() {
        assert_eq!(
            plan_name(&json!({"user":{"membership":{"level":"LEVEL_BASIC"}}})).as_deref(),
            Some("Basic")
        );
        assert_eq!(
            plan_name(&json!({"user":{"membership":{"level":"LEVEL_YEARLY_PRO"}}})).as_deref(),
            Some("Yearly Pro")
        );
        assert_eq!(plan_name(&json!({"user":{"membership":{}}})), None);
        assert_eq!(plan_name(&json!({})), None);
    }
}
