use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

use super::DevinError;

pub(super) const DAY_PERIOD_SECONDS: u64 = 24 * 60 * 60;
pub(super) const WEEK_PERIOD_SECONDS: u64 = 7 * DAY_PERIOD_SECONDS;

#[derive(Debug, PartialEq)]
pub(super) struct DevinMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    pub value_metrics: Vec<ValueMetric>,
}

pub(super) fn map_user_status_response(body: &Value) -> Result<DevinMappedUsage, DevinError> {
    let user_status = body
        .get("userStatus")
        .and_then(Value::as_object)
        .ok_or(DevinError::InvalidResponse)?;
    map_user_status(user_status)
}

fn map_user_status(
    user_status: &serde_json::Map<String, Value>,
) -> Result<DevinMappedUsage, DevinError> {
    let plan_status = user_status.get("planStatus").and_then(Value::as_object);
    let plan_info = plan_status
        .and_then(|status| status.get("planInfo"))
        .and_then(Value::as_object);
    let plan = plan_info
        .and_then(|info| info.get("planName"))
        .and_then(Value::as_str)
        .and_then(non_empty)
        .unwrap_or_else(|| "Unknown".into());
    let hide_daily = plan_info
        .and_then(|info| info.get("hideDailyQuota"))
        .and_then(bool_value)
        == Some(true);

    let daily_remaining = plan_status
        .and_then(|status| status.get("dailyQuotaRemainingPercent"))
        .and_then(number);
    let weekly_remaining = plan_status
        .and_then(|status| status.get("weeklyQuotaRemainingPercent"))
        .and_then(number);
    let daily_reset = (!hide_daily)
        .then(|| {
            plan_status
                .and_then(|status| status.get("dailyQuotaResetAtUnix"))
                .and_then(unix_seconds)
        })
        .flatten();
    let weekly_reset = plan_status
        .and_then(|status| status.get("weeklyQuotaResetAtUnix"))
        .and_then(unix_seconds);

    let mut quotas = Vec::new();
    if !hide_daily {
        if let Some(remaining) = daily_remaining {
            quotas.push(quota(
                "daily",
                "Daily",
                remaining,
                daily_reset,
                DAY_PERIOD_SECONDS,
            ));
        }
    }
    if let Some(remaining) = weekly_remaining {
        quotas.push(quota(
            "weekly",
            "Weekly",
            remaining,
            weekly_reset,
            WEEK_PERIOD_SECONDS,
        ));
    } else if hide_daily {
        if let Some(remaining) = daily_remaining {
            quotas.push(quota(
                "weekly",
                "Weekly",
                remaining,
                weekly_reset,
                WEEK_PERIOD_SECONDS,
            ));
        }
    }

    let balance = plan_status
        .and_then(|status| status.get("overageBalanceMicros"))
        .and_then(number)
        .map(|micros| micros.max(0.0) / 1_000_000.0);
    let value_metrics = balance
        .map(|balance| {
            vec![ValueMetric {
                id: "extraUsageBalance".into(),
                label: "Extra Usage Balance".into(),
                values: vec![MetricValue {
                    number: balance,
                    kind: MetricValueKind::Dollars,
                    label: None,
                    estimated: false,
                }],
                expiries_at: Vec::new(),
            }]
        })
        .unwrap_or_default();

    if quotas.is_empty() && value_metrics.is_empty() {
        return Err(DevinError::QuotaUnavailable);
    }
    Ok(DevinMappedUsage {
        plan: Some(plan),
        quotas,
        value_metrics,
    })
}

fn quota(
    id: &str,
    label: &str,
    remaining: f64,
    resets_at: Option<DateTime<Utc>>,
    period_seconds: u64,
) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: (100.0 - remaining).clamp(0.0, 100.0),
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    }
}

fn unix_seconds(value: &Value) -> Option<DateTime<Utc>> {
    let seconds = number(value)?;
    let milliseconds = seconds * 1_000.0;
    if milliseconds < i64::MIN as f64 || milliseconds > i64::MAX as f64 {
        return None;
    }
    DateTime::from_timestamp_millis(milliseconds.trunc() as i64)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<f64>().ok())
        })
        .filter(|value| value.is_finite())
}

fn bool_value(value: &Value) -> Option<bool> {
    if let Some(value) = value.as_bool() {
        return Some(value);
    }
    if let Some(value) = value.as_f64() {
        return Some(value != 0.0);
    }
    match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
