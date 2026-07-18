use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

use super::ZaiError;

pub const MONTHLY_PERIOD_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, PartialEq)]
pub struct ZaiMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
}

pub fn map_usage(
    quota_body: &Value,
    subscription_body: Option<&Value>,
) -> Result<ZaiMappedUsage, ZaiError> {
    Ok(ZaiMappedUsage {
        plan: subscription_body.and_then(plan_name),
        quotas: map_quota(quota_body)?,
    })
}

pub fn is_no_coding_plan(body: &Value) -> bool {
    body.get("success").and_then(Value::as_bool) == Some(false)
        && body
            .get("msg")
            .and_then(Value::as_str)
            .is_some_and(|message| message.to_ascii_lowercase().contains("coding plan"))
}

pub fn plan_name(body: &Value) -> Option<String> {
    body.get("data")
        .and_then(Value::as_array)?
        .first()?
        .get("productName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

pub fn map_quota(body: &Value) -> Result<Vec<QuotaWindow>, ZaiError> {
    let root = body.as_object().ok_or(ZaiError::InvalidResponse)?;
    let container = match root.get("data") {
        Some(data) => data.as_object().ok_or(ZaiError::InvalidResponse)?,
        None => root,
    };
    let limits = container
        .get("limits")
        .and_then(Value::as_array)
        .ok_or(ZaiError::InvalidResponse)?;
    if limits.is_empty() {
        return Ok(Vec::new());
    }
    let limits = limits
        .iter()
        .map(|entry| entry.as_object().ok_or(ZaiError::InvalidResponse))
        .collect::<Result<Vec<_>, _>>()?;

    let mut session = None;
    let mut weekly = None;
    for entry in limits
        .iter()
        .copied()
        .filter(|entry| matches_limit(entry, "TOKENS_LIMIT"))
    {
        let Some(window) = classify_token_window(entry)? else {
            continue;
        };
        let quota = percent_quota(entry, window)?;
        let target = match window.kind {
            TokenWindowKind::Session => &mut session,
            TokenWindowKind::Weekly => &mut weekly,
        };
        if target.replace(quota).is_some() {
            return Err(ZaiError::InvalidResponse);
        }
    }

    let web_searches = limits
        .iter()
        .copied()
        .find(|entry| matches_limit(entry, "TIME_LIMIT"))
        .map(web_search_quota)
        .transpose()?;

    Ok(session
        .into_iter()
        .chain(weekly)
        .chain(web_searches)
        .collect())
}

#[derive(Debug, Clone, Copy)]
enum TokenWindowKind {
    Session,
    Weekly,
}

#[derive(Debug, Clone, Copy)]
struct TokenWindow {
    kind: TokenWindowKind,
    period_seconds: u64,
}

fn classify_token_window(
    entry: &serde_json::Map<String, Value>,
) -> Result<Option<TokenWindow>, ZaiError> {
    let unit = number(entry.get("unit")).ok_or(ZaiError::InvalidResponse)?;
    let count = number(entry.get("number"))
        .filter(|value| *value > 0.0)
        .ok_or(ZaiError::InvalidResponse)?;
    let unit_seconds = match unit {
        3.0 => 60.0 * 60.0,
        4.0 => 24.0 * 60.0 * 60.0,
        5.0 => MONTHLY_PERIOD_SECONDS as f64,
        6.0 => 7.0 * 24.0 * 60.0 * 60.0,
        _ => return Ok(None),
    };
    let duration = unit_seconds * count;
    if !duration.is_finite() || duration < 1.0 || duration > u64::MAX as f64 {
        return Err(ZaiError::InvalidResponse);
    }
    let period_seconds = duration.trunc() as u64;
    Ok(Some(TokenWindow {
        kind: if period_seconds < 24 * 60 * 60 {
            TokenWindowKind::Session
        } else {
            TokenWindowKind::Weekly
        },
        period_seconds,
    }))
}

fn percent_quota(
    entry: &serde_json::Map<String, Value>,
    window: TokenWindow,
) -> Result<QuotaWindow, ZaiError> {
    let used_percent = number(entry.get("percentage"))
        .ok_or(ZaiError::InvalidResponse)?
        .clamp(0.0, 100.0);
    let (id, label) = match window.kind {
        TokenWindowKind::Session => ("session", "Session"),
        TokenWindowKind::Weekly => ("weekly", "Weekly"),
    };
    Ok(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent,
        resets_at: reset_time(entry.get("nextResetTime")),
        period_seconds: window.period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    })
}

fn web_search_quota(entry: &serde_json::Map<String, Value>) -> Result<QuotaWindow, ZaiError> {
    let used = number(entry.get("currentValue"))
        .filter(|value| *value >= 0.0)
        .ok_or(ZaiError::InvalidResponse)?;
    let limit = number(entry.get("usage"))
        .filter(|value| *value >= 0.0)
        .ok_or(ZaiError::InvalidResponse)?;
    Ok(QuotaWindow {
        id: "webSearches".into(),
        label: "Web Searches".into(),
        used_percent: if limit > 0.0 {
            (used / limit * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        resets_at: reset_time(entry.get("nextResetTime")),
        period_seconds: MONTHLY_PERIOD_SECONDS,
        format: QuotaFormat::Count,
        used_value: Some(used),
        limit_value: Some(limit),
        unit: Some("searches".into()),
        estimated: false,
        source_note: None,
    })
}

fn matches_limit(entry: &serde_json::Map<String, Value>, expected: &str) -> bool {
    entry.get("type").and_then(Value::as_str) == Some(expected)
        || entry.get("name").and_then(Value::as_str) == Some(expected)
}

fn reset_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let milliseconds = number(value)?;
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

    use super::{is_no_coding_plan, map_quota, map_usage, plan_name, MONTHLY_PERIOD_SECONDS};
    use crate::{models::QuotaFormat, providers::zai::ZaiError};

    fn captured_quota() -> Value {
        serde_json::from_str(include_str!("fixtures/quota.json")).unwrap()
    }

    fn captured_subscription() -> Value {
        serde_json::from_str(include_str!("fixtures/subscription.json")).unwrap()
    }

    #[test]
    fn captured_payload_maps_session_weekly_web_searches_and_plan() {
        let mapped = map_usage(&captured_quota(), Some(&captured_subscription())).unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("GLM Coding Pro"));
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly", "webSearches"]
        );
        let session = &mapped.quotas[0];
        assert_eq!(session.used_percent, 17.0);
        assert_eq!(session.period_seconds, 5 * 60 * 60);
        assert_eq!(
            session.resets_at,
            Utc.timestamp_millis_opt(1_782_724_971_179).single()
        );
        let weekly = &mapped.quotas[1];
        assert_eq!(weekly.period_seconds, 7 * 24 * 60 * 60);

        let web = &mapped.quotas[2];
        assert_eq!(web.format, QuotaFormat::Count);
        assert_eq!(web.used_value, Some(0.0));
        assert_eq!(web.limit_value, Some(1_000.0));
        assert_eq!(web.unit.as_deref(), Some("searches"));
        assert_eq!(web.period_seconds, MONTHLY_PERIOD_SECONDS);
        assert!(!web.estimated);
        assert_eq!(web.source_note, None);
    }

    #[test]
    fn payload_windows_drive_classification_and_percentages_are_clamped() {
        let mapped = map_quota(&json!({"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"number":3,"percentage":-10},
            {"type":"TOKENS_LIMIT","unit":4,"number":3,"percentage":"150"}
        ]}}))
        .unwrap();

        assert_eq!(mapped[0].id, "session");
        assert_eq!(mapped[0].period_seconds, 3 * 60 * 60);
        assert_eq!(mapped[0].used_percent, 0.0);
        assert_eq!(mapped[1].id, "weekly");
        assert_eq!(mapped[1].period_seconds, 3 * 24 * 60 * 60);
        assert_eq!(mapped[1].used_percent, 100.0);
    }

    #[test]
    fn explicit_zeroes_are_measurements_instead_of_missing_data() {
        let mapped = map_quota(&json!({"limits":[
            {"name":"TOKENS_LIMIT","unit":"3","number":"5","percentage":0},
            {"name":"TIME_LIMIT","currentValue":"0","usage":"0"}
        ]}))
        .unwrap();

        assert_eq!(mapped[0].used_percent, 0.0);
        assert_eq!(mapped[1].used_value, Some(0.0));
        assert_eq!(mapped[1].limit_value, Some(0.0));
        assert_eq!(mapped[1].used_percent, 0.0);
    }

    #[test]
    fn malformed_envelopes_and_partial_known_limits_fail_loudly() {
        for body in [
            Value::Null,
            json!({"data":[]}),
            json!({"data":{}}),
            json!({"data":{"limits":{}}}),
            json!({"data":{"limits":[null]}}),
            json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"number":5}]}}),
            json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"number":0,"percentage":5}]}}),
            json!({"data":{"limits":[{"type":"TIME_LIMIT","usage":1000}]}}),
            json!({"data":{"limits":[{"type":"TIME_LIMIT","currentValue":0}]}}),
            json!({"data":{"limits":[{"type":"TIME_LIMIT","currentValue":-1,"usage":1000}]}}),
        ] {
            assert!(matches!(map_quota(&body), Err(ZaiError::InvalidResponse)));
        }
        assert!(map_quota(&json!({"data":{"limits":[]}}))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unknown_limits_and_units_do_not_hide_known_meters() {
        let mapped = map_quota(&json!({"data":{"limits":[
            {"type":"FUTURE_LIMIT"},
            {"type":"TOKENS_LIMIT","unit":99,"number":1,"percentage":70},
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":25}
        ]}}))
        .unwrap();

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].id, "session");

        let unknown_only = map_quota(&json!({"data":{"limits":[
            {"type":"FUTURE_LIMIT"},
            {"type":"TOKENS_LIMIT","unit":99,"number":1,"percentage":70}
        ]}}))
        .unwrap();
        assert!(unknown_only.is_empty());
    }

    #[test]
    fn invalid_or_unknown_resets_are_omitted_without_losing_usage() {
        let mapped = map_quota(&json!({"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":25,
             "nextResetTime":"not-a-time"},
            {"type":"TIME_LIMIT","currentValue":1,"usage":10,
             "nextResetTime":"1785292686976"}
        ]}}))
        .unwrap();

        assert_eq!(mapped[0].resets_at, None);
        assert_eq!(
            mapped[1].resets_at,
            Utc.timestamp_millis_opt(1_785_292_686_976).single()
        );
    }

    #[test]
    fn no_plan_signal_and_optional_subscription_are_structurally_checked() {
        assert!(is_no_coding_plan(&json!({
            "success":false,
            "code":500,
            "msg":"Current user does not have a coding plan"
        })));
        assert!(!is_no_coding_plan(&json!({
            "success":false,
            "msg":"internal error"
        })));
        assert!(!is_no_coding_plan(&json!({
            "success":"false",
            "msg":"coding plan"
        })));

        assert_eq!(
            plan_name(&json!({"data":[{"productName":" GLM Coding Max "}]})).as_deref(),
            Some("GLM Coding Max")
        );
        assert_eq!(plan_name(&json!({"data":[]})), None);
        assert_eq!(plan_name(&json!({"data":[{"productName":42}]})), None);
    }

    #[test]
    fn duplicate_known_windows_are_rejected_before_snapshot_validation() {
        let error = map_quota(&json!({"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":10},
            {"type":"TOKENS_LIMIT","unit":3,"number":3,"percentage":20}
        ]}}))
        .unwrap_err();

        assert!(matches!(error, ZaiError::InvalidResponse));
    }
}
