use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

use super::{client::UsageResponse, CodexError};

pub struct MappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
}

pub fn map_usage(response: &UsageResponse, now: DateTime<Utc>) -> Result<MappedUsage, CodexError> {
    if response.status.as_u16() == 401 || response.status.as_u16() == 403 {
        return Err(CodexError::TokenExpired);
    }
    if !response.status.is_success() {
        return Err(CodexError::RequestFailed(response.status.as_u16()));
    }
    let rate_limit = response.body.get("rate_limit");
    let primary = rate_limit.and_then(|value| value.get("primary_window"));
    let secondary = rate_limit.and_then(|value| value.get("secondary_window"));
    let mut quotas = Vec::new();

    if let Some(window) = map_window(
        "session",
        "Session",
        primary,
        header_number(response, "x-codex-primary-used-percent"),
        5 * 60 * 60,
        now,
    ) {
        quotas.push(window);
    }
    if let Some(window) = map_window(
        "weekly",
        "Weekly",
        secondary,
        header_number(response, "x-codex-secondary-used-percent"),
        7 * 24 * 60 * 60,
        now,
    ) {
        quotas.push(window);
    }

    Ok(MappedUsage {
        plan: format_plan(response.body.get("plan_type")),
        quotas,
    })
}

fn map_window(
    id: &str,
    label: &str,
    value: Option<&Value>,
    header_fallback: Option<f64>,
    default_period: u64,
    now: DateTime<Utc>,
) -> Option<QuotaWindow> {
    let used_percent = value
        .and_then(|window| number(window.get("used_percent")))
        .or(header_fallback)?;
    let resets_at = value.and_then(|window| {
        number(window.get("reset_at"))
            .and_then(|seconds| DateTime::from_timestamp(seconds as i64, 0))
            .or_else(|| {
                number(window.get("reset_after_seconds"))
                    .map(|seconds| now + Duration::seconds(seconds as i64))
            })
    });
    let period_seconds = value
        .and_then(|window| number(window.get("limit_window_seconds")))
        .map(|value| value.max(0.0) as u64)
        .unwrap_or(default_period);
    Some(QuotaWindow {
        id: id.to_owned(),
        label: label.to_owned(),
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
    })
}

fn header_number(response: &UsageResponse, name: &str) -> Option<f64> {
    response
        .headers
        .get(name)
        .and_then(|value| value.parse().ok())
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}

fn format_plan(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(match raw.to_ascii_lowercase().as_str() {
        "prolite" => "Pro 5x".to_owned(),
        "pro" => "Pro 20x".to_owned(),
        _ => raw
            .split('_')
            .map(|part| {
                let mut characters = part.chars();
                characters
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use reqwest::StatusCode;
    use serde_json::json;

    use super::map_usage;
    use crate::providers::codex::client::UsageResponse;

    #[test]
    fn maps_body_windows_before_header_fallbacks() {
        let response = UsageResponse {
            status: StatusCode::OK,
            headers: HashMap::from([
                ("x-codex-primary-used-percent".into(), "99".into()),
                ("x-codex-secondary-used-percent".into(), "50".into()),
            ]),
            body: json!({
                "plan_type": "prolite",
                "rate_limit": {
                    "primary_window": {"used_percent": 10, "reset_after_seconds": 60},
                    "secondary_window": {"used_percent": 20, "reset_after_seconds": 120}
                }
            }),
        };
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mapped = map_usage(&response, now).unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Pro 5x"));
        assert_eq!(mapped.quotas[0].used_percent, 10.0);
        assert_eq!(mapped.quotas[1].used_percent, 20.0);
        assert_eq!(mapped.quotas[0].period_seconds, 5 * 60 * 60);
    }

    #[test]
    fn provider_fixture_maps_subscription_windows_and_resets() {
        let response = UsageResponse {
            status: StatusCode::OK,
            headers: HashMap::new(),
            body: serde_json::from_str(include_str!("../../../tests/fixtures/codex_usage.json"))
                .unwrap(),
        };
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mapped = map_usage(&response, now).unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Plus"));
        assert_eq!(mapped.quotas.len(), 2);
        assert_eq!(mapped.quotas[0].used_percent, 37.5);
        assert_eq!(mapped.quotas[1].period_seconds, 604_800);
        assert!(mapped.quotas.iter().all(|quota| quota.resets_at.is_some()));
    }
}
