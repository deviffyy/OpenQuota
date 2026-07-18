use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow, StatusMetric, StatusTone};

use super::{client::GrokResponse, GrokError};

const WEEKLY_PERIOD_TYPE: &str = "USAGE_PERIOD_TYPE_WEEKLY";

#[derive(Debug, PartialEq)]
pub struct GrokMetrics {
    pub quotas: Vec<QuotaWindow>,
    pub status_metrics: Vec<StatusMetric>,
}

#[derive(Debug, PartialEq)]
struct CreditsConfig {
    period_type: String,
    used_percent: f64,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    on_demand_cap: f64,
}

pub fn map_credits(response: &GrokResponse) -> Result<GrokMetrics, GrokError> {
    require_success(response.status)?;
    let config = decode_credits(&response.body)?;
    let status_metrics = vec![StatusMetric {
        id: "payAsYouGo".into(),
        label: "Pay as you go".into(),
        text: if config.on_demand_cap > 0.0 {
            format!("{} cap", format_units(config.on_demand_cap))
        } else {
            "Disabled".into()
        },
        tone: if config.on_demand_cap > 0.0 {
            StatusTone::Positive
        } else {
            StatusTone::Neutral
        },
        subtitle: None,
    }];
    let quotas = (config.period_type == WEEKLY_PERIOD_TYPE)
        .then(|| QuotaWindow {
            id: "weekly".into(),
            label: "Weekly".into(),
            used_percent: config.used_percent.clamp(0.0, 100.0),
            resets_at: Some(config.period_end),
            period_seconds: config
                .period_end
                .signed_duration_since(config.period_start)
                .num_seconds() as u64,
            format: QuotaFormat::Percent,
            used_value: None,
            limit_value: None,
            unit: None,
            estimated: false,
            source_note: None,
        })
        .into_iter()
        .collect();
    Ok(GrokMetrics {
        quotas,
        status_metrics,
    })
}

pub fn plan_name(response: &GrokResponse) -> Option<String> {
    if !response.status.is_success() {
        return None;
    }
    response
        .body
        .get("subscription_tier_display")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn require_success(status: StatusCode) -> Result<(), GrokError> {
    if status.is_success() {
        Ok(())
    } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        Err(GrokError::Expired)
    } else {
        Err(GrokError::RequestFailed(status.as_u16()))
    }
}

fn decode_credits(body: &Value) -> Result<CreditsConfig, GrokError> {
    let config = body
        .get("config")
        .and_then(Value::as_object)
        .ok_or(GrokError::InvalidResponse)?;
    let period = config
        .get("currentPeriod")
        .and_then(Value::as_object)
        .ok_or(GrokError::InvalidResponse)?;
    let period_type = period
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GrokError::InvalidResponse)?
        .to_owned();
    let period_start = date(period.get("start")).ok_or(GrokError::InvalidResponse)?;
    let period_end = date(period.get("end")).ok_or(GrokError::InvalidResponse)?;
    if period_end <= period_start {
        return Err(GrokError::InvalidResponse);
    }

    let used_percent = match config.get("creditUsagePercent") {
        Some(value) => finite_number(value).ok_or(GrokError::InvalidResponse)?,
        None => 0.0,
    };
    let on_demand_cap = match config.get("onDemandCap") {
        Some(value) => {
            let object = value.as_object().ok_or(GrokError::InvalidResponse)?;
            match object.get("val") {
                Some(value) => finite_number(value).ok_or(GrokError::InvalidResponse)?,
                None => 0.0,
            }
        }
        None => 0.0,
    };

    Ok(CreditsConfig {
        period_type,
        used_percent,
        period_start,
        period_end,
        on_demand_cap,
    })
}

fn date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?.as_str()?.trim())
        .ok()
        .map(|date| date.to_utc())
}

fn finite_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value: &f64| value.is_finite())
}

fn format_units(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::{json, Value};

    use super::{map_credits, plan_name, GrokResponse};
    use crate::{models::StatusTone, providers::grok::GrokError};

    fn response(body: Value) -> GrokResponse {
        GrokResponse {
            status: StatusCode::OK,
            body,
        }
    }

    fn captured() -> Value {
        serde_json::from_str(include_str!("fixtures/credits.json")).unwrap()
    }

    fn shaped(period_type: &str, percent: Option<Value>, cap: Option<Value>) -> Value {
        let mut config = json!({
            "currentPeriod": {
                "type": period_type,
                "start": "2026-06-30T21:36:52.140114+00:00",
                "end": "2026-07-07T21:36:52.140114+00:00"
            }
        });
        if let Some(percent) = percent {
            config["creditUsagePercent"] = percent;
        }
        if let Some(cap) = cap {
            config["onDemandCap"] = json!({"val": cap});
        }
        json!({"config": config})
    }

    #[test]
    fn captured_weekly_payload_maps_meter_and_disabled_status() {
        let mapped = map_credits(&response(captured())).unwrap();

        assert_eq!(mapped.quotas.len(), 1);
        let weekly = &mapped.quotas[0];
        assert_eq!(weekly.id, "weekly");
        assert_eq!(weekly.used_percent, 99.0);
        assert_eq!(weekly.period_seconds, 7 * 24 * 60 * 60);
        assert_eq!(
            weekly.resets_at.unwrap().to_rfc3339(),
            "2026-07-07T21:36:52.140114+00:00"
        );
        assert_eq!(mapped.status_metrics[0].text, "Disabled");
        assert_eq!(mapped.status_metrics[0].tone, StatusTone::Neutral);
    }

    #[test]
    fn enabled_cap_maps_to_positive_status() {
        let mapped = map_credits(&response(shaped(
            "USAGE_PERIOD_TYPE_WEEKLY",
            Some(json!(25)),
            Some(json!(2500)),
        )))
        .unwrap();

        assert_eq!(mapped.status_metrics[0].text, "2500 cap");
        assert_eq!(mapped.status_metrics[0].tone, StatusTone::Positive);
    }

    #[test]
    fn a_monthly_period_is_not_mislabeled_as_weekly() {
        let mapped = map_credits(&response(shaped(
            "USAGE_PERIOD_TYPE_MONTHLY",
            Some(json!(25)),
            None,
        )))
        .unwrap();

        assert!(mapped.quotas.is_empty());
        assert_eq!(mapped.status_metrics[0].text, "Disabled");
    }

    #[test]
    fn omitted_proto_zeroes_are_real_zeroes_and_percent_is_clamped() {
        let omitted =
            map_credits(&response(shaped("USAGE_PERIOD_TYPE_WEEKLY", None, None))).unwrap();
        assert_eq!(omitted.quotas[0].used_percent, 0.0);

        let high = map_credits(&response(shaped(
            "USAGE_PERIOD_TYPE_WEEKLY",
            Some(json!(150)),
            None,
        )))
        .unwrap();
        assert_eq!(high.quotas[0].used_percent, 100.0);
    }

    #[test]
    fn schema_drift_and_invalid_periods_fail_loudly() {
        for body in [
            json!({}),
            json!({"config": {}}),
            shaped("USAGE_PERIOD_TYPE_WEEKLY", Some(json!("high")), None),
            shaped(
                "USAGE_PERIOD_TYPE_WEEKLY",
                Some(json!(10)),
                Some(json!("many")),
            ),
            json!({"config": {
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-07-07T00:00:00Z",
                    "end": "2026-07-01T00:00:00Z"
                }
            }}),
        ] {
            assert!(matches!(
                map_credits(&response(body)),
                Err(GrokError::InvalidResponse)
            ));
        }
    }

    #[test]
    fn auth_and_other_http_failures_stay_distinct() {
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            assert!(matches!(
                map_credits(&GrokResponse {
                    status,
                    body: Value::Null
                }),
                Err(GrokError::Expired)
            ));
        }
        assert!(matches!(
            map_credits(&GrokResponse {
                status: StatusCode::SERVICE_UNAVAILABLE,
                body: Value::Null
            }),
            Err(GrokError::RequestFailed(503))
        ));
    }

    #[test]
    fn settings_plan_is_optional_and_trimmed() {
        assert_eq!(
            plan_name(&response(json!({"subscription_tier_display": " Heavy "}))).as_deref(),
            Some("Heavy")
        );
        assert_eq!(plan_name(&response(json!({}))), None);
        assert_eq!(
            plan_name(&GrokResponse {
                status: StatusCode::FORBIDDEN,
                body: json!({"subscription_tier_display": "Hidden"})
            }),
            None
        );
    }
}
