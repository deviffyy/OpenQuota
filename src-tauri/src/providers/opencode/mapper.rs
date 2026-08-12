use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

use super::{client::UsageResponse, OpenCodeError};

pub(super) fn map_go_usage(response: UsageResponse) -> Result<Vec<QuotaWindow>, OpenCodeError> {
    match response.status.as_u16() {
        200..=299 => {}
        401 | 403 => return Err(OpenCodeError::InvalidAuth),
        status => return Err(OpenCodeError::RequestFailed(status)),
    }
    let usage = response
        .body
        .get("usage")
        .and_then(Value::as_object)
        .ok_or(OpenCodeError::InvalidResponse)?;
    [
        quota(usage.get("rolling"), "session", "Session"),
        quota(usage.get("weekly"), "weekly", "Weekly"),
        quota(usage.get("monthly"), "monthly", "Monthly"),
    ]
    .into_iter()
    .collect()
}

fn quota(value: Option<&Value>, id: &str, label: &str) -> Result<QuotaWindow, OpenCodeError> {
    let value = value
        .and_then(Value::as_object)
        .ok_or(OpenCodeError::InvalidResponse)?;
    let used_percent = value
        .get("percent")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or(OpenCodeError::InvalidResponse)?
        .clamp(0.0, 100.0);
    let resets_at = value
        .get("resetsAt")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    Ok(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent,
        resets_at,
        period_seconds: 0,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    })
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::{map_go_usage, UsageResponse};

    #[test]
    fn maps_authoritative_go_usage_windows() {
        let response = UsageResponse {
            status: StatusCode::OK,
            body: json!({"usage": {
                "rolling": {"percent": 31, "resetsAt": "2026-08-12T12:00:00Z", "status": "active"},
                "weekly": {"percent": 100, "resetsAt": "2026-08-17T00:00:00Z", "status": "exhausted"},
                "monthly": {"percent": 72, "resetsAt": "2026-09-05T00:00:00Z", "status": "active"}
            }}),
        };
        let quotas = map_go_usage(response).unwrap();
        assert_eq!(quotas.len(), 3);
        assert_eq!(quotas[0].id, "session");
        assert_eq!(quotas[1].used_percent, 100.0);
        assert!(!quotas.iter().any(|quota| quota.estimated));
    }
}
