use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

use super::{client::EndpointResponse, CommandCodeError};

#[derive(Debug, PartialEq)]
pub struct CommandCodeMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    pub value_metrics: Vec<ValueMetric>,
}

pub fn map_usage(
    credits: &EndpointResponse,
    subscription: &EndpointResponse,
) -> Result<CommandCodeMappedUsage, CommandCodeError> {
    require_success(credits)?;
    require_success(subscription)?;
    let windows = credits
        .body
        .get("windowLimits")
        .and_then(Value::as_object)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let quotas = [
        quota(windows.get("fiveHour"), "session", "Session", 5 * 60 * 60),
        quota(windows.get("weekly"), "weekly", "Weekly", 7 * 24 * 60 * 60),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let credits_body = credits
        .body
        .get("credits")
        .and_then(Value::as_object)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let mut values = Vec::new();
    if let Some(monthly) = number(credits_body.get("monthlyCredits")) {
        values.push(dollars_metric("monthlyCredits", "Monthly Credits", monthly));
    }
    if let Some(purchased) =
        number(credits_body.get("purchasedCredits")).filter(|value| *value > 0.0)
    {
        values.push(dollars_metric("extraCredits", "Extra Credits", purchased));
    }
    Ok(CommandCodeMappedUsage {
        plan: subscription
            .body
            .get("data")
            .and_then(|data| data.get("planId"))
            .and_then(Value::as_str)
            .map(display_plan),
        quotas,
        value_metrics: values,
    })
}

fn require_success(response: &EndpointResponse) -> Result<(), CommandCodeError> {
    if response.status.is_success() {
        Ok(())
    } else if response.status.as_u16() == 401 || response.status.as_u16() == 403 {
        Err(CommandCodeError::InvalidAuth)
    } else {
        Err(CommandCodeError::RequestFailed(response.status.as_u16()))
    }
}

fn quota(
    value: Option<&Value>,
    id: &str,
    label: &str,
    period_seconds: u64,
) -> Result<QuotaWindow, CommandCodeError> {
    let value = value
        .and_then(Value::as_object)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let cap = number(value.get("cap"))
        .filter(|cap| *cap > 0.0)
        .ok_or(CommandCodeError::InvalidResponse)?;
    let used = number(value.get("used"))
        .filter(|used| *used >= 0.0)
        .ok_or(CommandCodeError::InvalidResponse)?;
    Ok(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: (used / cap * 100.0).clamp(0.0, 100.0),
        resets_at: value.get("resetAt").and_then(iso_time),
        period_seconds,
        format: QuotaFormat::Dollars,
        used_value: Some(used.min(cap)),
        limit_value: Some(cap),
        unit: None,
        estimated: false,
        source_note: None,
    })
}

fn dollars_metric(id: &str, label: &str, amount: f64) -> ValueMetric {
    ValueMetric {
        id: id.into(),
        label: label.into(),
        values: vec![MetricValue {
            number: amount.max(0.0),
            kind: MetricValueKind::Dollars,
            label: Some("remaining".into()),
            estimated: false,
        }],
        expiries_at: Vec::new(),
    }
}

fn display_plan(value: &str) -> String {
    value
        .rsplit('-')
        .next()
        .unwrap_or(value)
        .replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
        })
        .filter(|value| value.is_finite())
}

fn iso_time(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::{map_usage, EndpointResponse};

    #[test]
    fn maps_live_window_limits_and_credit_balances() {
        let credits = EndpointResponse {
            status: StatusCode::OK,
            body: json!({
                "credits": {"monthlyCredits": 7.5, "purchasedCredits": 2.0},
                "windowLimits": {
                    "fiveHour": {"cap": 3, "used": 0.75, "resetAt": "2026-08-10T12:00:00Z"},
                    "weekly": {"cap": 6, "used": 3, "resetAt": "2026-08-15T12:00:00Z"}
                }
            }),
        };
        let subscription = EndpointResponse {
            status: StatusCode::OK,
            body: json!({"success": true, "data": {"planId": "individual-goat"}}),
        };

        let mapped = map_usage(&credits, &subscription).unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Goat"));
        assert_eq!(mapped.quotas[0].used_percent, 25.0);
        assert_eq!(mapped.quotas[1].used_percent, 50.0);
        assert_eq!(mapped.value_metrics.len(), 2);
        assert_eq!(mapped.value_metrics[0].id, "monthlyCredits");
        assert_eq!(mapped.value_metrics[0].values[0].number, 7.5);
    }
}
