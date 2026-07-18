use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

#[derive(Debug, Default, PartialEq)]
pub struct CreditsMetrics {
    pub quota: Option<QuotaWindow>,
    pub balance: Option<ValueMetric>,
}

#[derive(Debug, Default, PartialEq)]
pub struct KeyMetrics {
    pub plan: Option<String>,
    pub quota: Option<QuotaWindow>,
    pub values: Vec<ValueMetric>,
}

pub fn data_object(body: &Value) -> Option<&serde_json::Map<String, Value>> {
    body.get("data")?.as_object()
}

pub fn map_credits(data: &serde_json::Map<String, Value>) -> CreditsMetrics {
    let Some(total_usage) = number(data.get("total_usage")) else {
        return CreditsMetrics::default();
    };
    let used = total_usage.max(0.0);
    let total = number(data.get("total_credits")).unwrap_or(0.0).max(0.0);
    CreditsMetrics {
        quota: (total > 0.0).then(|| dollars_quota("credits", "Credits", used, total)),
        balance: Some(dollars_value("balance", "Balance", (total - used).max(0.0))),
    }
}

pub fn map_key(data: &serde_json::Map<String, Value>) -> KeyMetrics {
    let values = [
        ("today", "Today", "usage_daily"),
        ("week", "This Week", "usage_weekly"),
        ("month", "This Month", "usage_monthly"),
    ]
    .into_iter()
    .filter_map(|(id, label, field)| {
        number(data.get(field)).map(|value| dollars_value(id, label, value.max(0.0)))
    })
    .collect();
    let quota = number(data.get("limit"))
        .filter(|limit| *limit > 0.0)
        .map(|limit| {
            dollars_quota(
                "keyLimit",
                "Key Limit",
                number(data.get("usage")).unwrap_or(0.0).max(0.0),
                limit,
            )
        });
    let plan = data
        .get("is_free_tier")
        .and_then(Value::as_bool)
        .map(|free| {
            if free {
                "Free tier".to_owned()
            } else {
                "Pay as you go".to_owned()
            }
        });
    KeyMetrics {
        plan,
        quota,
        values,
    }
}

fn dollars_quota(id: &str, label: &str, used: f64, limit: f64) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: if limit > 0.0 {
            (used / limit * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        resets_at: None,
        period_seconds: 0,
        format: QuotaFormat::Dollars,
        used_value: Some(used),
        limit_value: Some(limit),
        unit: None,
        estimated: false,
        source_note: None,
    }
}

fn dollars_value(id: &str, label: &str, number: f64) -> ValueMetric {
    ValueMetric {
        id: id.into(),
        label: label.into(),
        values: vec![MetricValue {
            number,
            kind: MetricValueKind::Dollars,
            label: None,
            estimated: false,
        }],
        expiries_at: Vec::new(),
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{data_object, map_credits, map_key};

    #[test]
    fn credits_map_to_a_dollar_meter_and_measured_balance() {
        let body = json!({"data":{"total_credits":277.47,"total_usage":178.20}});
        let mapped = map_credits(data_object(&body).unwrap());
        let quota = mapped.quota.unwrap();
        assert_eq!(quota.id, "credits");
        assert_eq!(quota.used_value, Some(178.20));
        assert_eq!(quota.limit_value, Some(277.47));
        assert!((mapped.balance.unwrap().values[0].number - 99.27).abs() < 0.001);
    }

    #[test]
    fn zero_purchased_credits_keep_balance_without_a_fake_meter() {
        let body = json!({"data":{"total_credits":0,"total_usage":0}});
        let mapped = map_credits(data_object(&body).unwrap());
        assert!(mapped.quota.is_none());
        assert_eq!(mapped.balance.unwrap().values[0].number, 0.0);
    }

    #[test]
    fn key_metadata_maps_period_spend_plan_and_optional_cap() {
        let body = json!({"data":{
            "is_free_tier":false,
            "usage_daily":0,
            "usage_weekly":1.25,
            "usage_monthly":"4.5",
            "usage":2,
            "limit":5
        }});
        let mapped = map_key(data_object(&body).unwrap());
        assert_eq!(mapped.plan.as_deref(), Some("Pay as you go"));
        assert_eq!(
            mapped
                .values
                .iter()
                .map(|metric| (metric.id.as_str(), metric.values[0].number))
                .collect::<Vec<_>>(),
            [("today", 0.0), ("week", 1.25), ("month", 4.5)]
        );
        assert_eq!(mapped.quota.unwrap().used_value, Some(2.0));
    }
}
