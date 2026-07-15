use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

use super::{auth::ClaudeOAuth, ClaudeError};

pub struct ClaudeMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    pub value_metrics: Vec<ValueMetric>,
}

pub fn map_usage(
    status: StatusCode,
    body: &Value,
    credentials: &ClaudeOAuth,
) -> Result<ClaudeMappedUsage, ClaudeError> {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(ClaudeError::TokenExpired);
    }
    if !status.is_success() {
        return Err(ClaudeError::RequestFailed(status.as_u16()));
    }
    let object = body.as_object().ok_or(ClaudeError::InvalidResponse)?;
    let mut quotas = Vec::new();
    let mut value_metrics = Vec::new();
    append_window(
        &mut quotas,
        "session",
        "Session",
        object.get("five_hour"),
        5 * 60 * 60,
    );
    append_window(
        &mut quotas,
        "weekly",
        "Weekly",
        object.get("seven_day"),
        7 * 24 * 60 * 60,
    );
    append_window(
        &mut quotas,
        "sonnet",
        "Sonnet",
        object.get("seven_day_sonnet"),
        7 * 24 * 60 * 60,
    );
    if let Some(limits) = object.get("limits").and_then(Value::as_array) {
        if let Some(limit) = limits.iter().find(|limit| {
            limit.get("kind").and_then(Value::as_str) == Some("weekly_scoped")
                && limit
                    .pointer("/scope/model/display_name")
                    .and_then(Value::as_str)
                    == Some("Fable")
        }) {
            append_percent(
                &mut quotas,
                "fable",
                "Fable",
                limit.get("percent").and_then(number),
                limit.get("resets_at").and_then(reset_date),
                7 * 24 * 60 * 60,
            );
        }
    }
    if let Some(extra) = object.get("extra_usage").and_then(Value::as_object) {
        if extra.get("is_enabled").and_then(Value::as_bool) == Some(true) {
            if let Some(used_cents) = extra.get("used_credits").and_then(number) {
                let used = used_cents / 100.0;
                let limit = extra
                    .get("monthly_limit")
                    .and_then(number)
                    .map(|v| v / 100.0);
                if limit.is_some_and(|value| value > 0.0) {
                    let limit_value = limit.unwrap_or_default();
                    quotas.push(QuotaWindow {
                        id: "extra".into(),
                        label: "Extra Usage".into(),
                        used_percent: (used / limit_value * 100.0).clamp(0.0, 100.0),
                        resets_at: None,
                        period_seconds: 0,
                        format: QuotaFormat::Dollars,
                        used_value: Some(used),
                        limit_value: Some(limit_value),
                    });
                } else if used > 0.0 {
                    value_metrics.push(ValueMetric {
                        id: "extra".into(),
                        label: "Extra Usage".into(),
                        values: vec![MetricValue {
                            number: used,
                            kind: MetricValueKind::Dollars,
                            label: Some("spent".into()),
                        }],
                        expiries_at: Vec::new(),
                    });
                }
            }
        }
    }
    Ok(ClaudeMappedUsage {
        plan: format_plan(
            credentials.subscription_type.as_deref(),
            credentials.rate_limit_tier.as_deref(),
        ),
        quotas,
        value_metrics,
    })
}

fn append_window(
    output: &mut Vec<QuotaWindow>,
    id: &str,
    label: &str,
    value: Option<&Value>,
    period_seconds: u64,
) {
    let Some(object) = value.and_then(Value::as_object) else {
        return;
    };
    append_percent(
        output,
        id,
        label,
        object.get("utilization").and_then(number),
        object.get("resets_at").and_then(reset_date),
        period_seconds,
    );
}

fn append_percent(
    output: &mut Vec<QuotaWindow>,
    id: &str,
    label: &str,
    percent: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
    period_seconds: u64,
) {
    let Some(percent) = percent.filter(|value| value.is_finite()) else {
        return;
    };
    output.push(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: percent,
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
    });
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn reset_date(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text.trim())
            .ok()
            .map(|date| date.to_utc())
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%dT%H:%M:%S%.f")
                    .ok()
                    .map(|date| date.and_utc())
            });
    }
    let raw = number(value)?;
    let seconds = if raw.abs() < 10_000_000_000.0 {
        raw
    } else {
        raw / 1000.0
    };
    DateTime::from_timestamp_millis((seconds * 1000.0) as i64)
}

fn format_plan(subscription: Option<&str>, tier: Option<&str>) -> Option<String> {
    let subscription = subscription?.trim();
    if subscription.is_empty() {
        return None;
    }
    let mut plan = subscription
        .to_ascii_lowercase()
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(multiplier) = tier.and_then(|tier| {
        tier.split(|character: char| !character.is_ascii_alphanumeric())
            .find(|part| part.ends_with('x') && part[..part.len() - 1].parse::<u32>().is_ok())
    }) {
        plan.push(' ');
        plan.push_str(multiplier);
    }
    Some(plan)
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::Value;

    use super::map_usage;
    use crate::providers::claude::auth::ClaudeOAuth;

    #[test]
    fn maps_live_windows_scoped_limit_and_extra_usage() {
        let body: Value =
            serde_json::from_str(include_str!("fixtures/usage.json")).expect("valid fixture");
        let mapped = map_usage(
            StatusCode::OK,
            &body,
            &ClaudeOAuth {
                subscription_type: Some("pro".into()),
                rate_limit_tier: Some("default_5x".into()),
                ..ClaudeOAuth::default()
            },
        )
        .unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Pro 5x"));
        assert_eq!(mapped.quotas.len(), 5);
        assert_eq!(mapped.quotas[4].used_percent, 25.0);
        assert_eq!(mapped.quotas[4].used_value, Some(12.5));
        assert!(mapped.value_metrics.is_empty());
    }

    #[test]
    fn maps_uncapped_extra_usage_as_an_unbounded_value() {
        let body = serde_json::json!({
            "extra_usage": {
                "is_enabled": true,
                "used_credits": 123456,
                "monthly_limit": null
            }
        });
        let mapped = map_usage(StatusCode::OK, &body, &ClaudeOAuth::default()).unwrap();
        assert!(mapped.quotas.is_empty());
        assert_eq!(mapped.value_metrics[0].id, "extra");
        assert_eq!(mapped.value_metrics[0].values[0].number, 1234.56);
        assert_eq!(
            mapped.value_metrics[0].values[0].label.as_deref(),
            Some("spent")
        );
    }

    #[test]
    fn maps_microsecond_reset_without_a_timezone_as_utc() {
        let body = serde_json::json!({
            "five_hour": {
                "utilization": 0,
                "resets_at": "2099-06-01T12:00:00.123456"
            }
        });
        let mapped = map_usage(StatusCode::OK, &body, &ClaudeOAuth::default()).unwrap();
        assert_eq!(
            mapped.quotas[0].resets_at.unwrap().to_rfc3339(),
            "2099-06-01T12:00:00.123456+00:00"
        );
    }
}
