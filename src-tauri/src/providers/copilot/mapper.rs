use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

use super::CopilotError;

pub(super) const MONTHLY_PERIOD_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, PartialEq)]
pub(super) struct CopilotMappedUsage {
    pub(super) plan: Option<String>,
    pub(super) quotas: Vec<QuotaWindow>,
    pub(super) value_metrics: Vec<ValueMetric>,
    pub(super) is_org_managed_seat: bool,
}

pub(super) fn map_usage(body: &Value) -> Result<CopilotMappedUsage, CopilotError> {
    let body = body.as_object().ok_or(CopilotError::InvalidResponse)?;
    let plan = body.get("copilot_plan").and_then(plan_label);
    let resets_at = body
        .get("quota_reset_date")
        .and_then(reset_date)
        .or_else(|| body.get("limited_user_reset_date").and_then(reset_date));

    let snapshots = body.get("quota_snapshots").and_then(Value::as_object);
    let premium = snapshots.and_then(|snapshots| {
        snapshots
            .get("premium_interactions")
            .or_else(|| snapshots.get("premium_requests"))
    });

    let mut quotas = Vec::new();
    let mut value_metrics = Vec::new();
    let credits =
        premium.and_then(|value| snapshot_quota("premium", "Credits", "credits", value, resets_at));
    if let Some(credits) = credits {
        quotas.push(credits);
        if let Some(extra) = premium.and_then(overage_metric) {
            value_metrics.push(extra);
        }
    }

    if let Some(snapshot) = snapshots.and_then(|snapshots| snapshots.get("chat")) {
        if let Some(quota) = snapshot_quota("chat", "Chat", "requests", snapshot, resets_at) {
            quotas.push(quota);
        }
    }
    if let Some(snapshot) = snapshots.and_then(|snapshots| snapshots.get("completions")) {
        if let Some(quota) = snapshot_quota(
            "completions",
            "Completions",
            "completions",
            snapshot,
            resets_at,
        ) {
            quotas.push(quota);
        }
    }

    if quotas.is_empty() && value_metrics.is_empty() {
        let limited = body.get("limited_user_quotas").and_then(Value::as_object);
        let monthly = body.get("monthly_quotas").and_then(Value::as_object);
        if let Some(quota) = legacy_quota(
            "chat",
            "Chat",
            "requests",
            limited.and_then(|value| value.get("chat")),
            monthly.and_then(|value| value.get("chat")),
            resets_at,
        ) {
            quotas.push(quota);
        }
        if let Some(quota) = legacy_quota(
            "completions",
            "Completions",
            "completions",
            limited.and_then(|value| value.get("completions")),
            monthly.and_then(|value| value.get("completions")),
            resets_at,
        ) {
            quotas.push(quota);
        }
    }

    if quotas.is_empty() && value_metrics.is_empty() {
        if bool_value(body.get("token_based_billing")) == Some(true) {
            return Ok(CopilotMappedUsage {
                plan,
                quotas,
                value_metrics,
                is_org_managed_seat: true,
            });
        }
        return Err(CopilotError::QuotaUnavailable);
    }

    Ok(CopilotMappedUsage {
        plan,
        quotas,
        value_metrics,
        is_org_managed_seat: false,
    })
}

pub(super) fn org_logins(body: &Value) -> Vec<String> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("login").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 100
                && value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
        .map(str::to_owned)
        .collect()
}

pub(super) fn map_org_usage(body: &Value) -> Option<Vec<ValueMetric>> {
    let items = body.get("usageItems")?.as_array()?;
    let credit_items = items.iter().filter(|item| {
        item.get("product")
            .and_then(Value::as_str)
            .is_some_and(|product| product.trim().eq_ignore_ascii_case("copilot"))
            && item
                .get("unitType")
                .and_then(Value::as_str)
                .is_some_and(|unit| {
                    matches!(
                        unit.trim().to_ascii_lowercase().as_str(),
                        "ai-units" | "ai-credits"
                    )
                })
    });

    let mut matched = false;
    let mut credits = 0.0;
    let mut spend = 0.0;
    for item in credit_items {
        matched = true;
        credits += number(item.get("grossQuantity")).unwrap_or(0.0).max(0.0);
        spend += number(item.get("netAmount")).unwrap_or(0.0).max(0.0);
    }
    matched.then(|| {
        vec![
            ValueMetric {
                id: "orgCredits".into(),
                label: "Org Credits".into(),
                values: vec![MetricValue {
                    number: credits,
                    kind: MetricValueKind::Count,
                    label: Some("credits".into()),
                    estimated: false,
                }],
                expiries_at: Vec::new(),
            },
            ValueMetric {
                id: "orgSpend".into(),
                label: "Org Spend".into(),
                values: vec![MetricValue {
                    number: spend,
                    kind: MetricValueKind::Dollars,
                    label: None,
                    estimated: false,
                }],
                expiries_at: Vec::new(),
            },
        ]
    })
}

fn snapshot_quota(
    id: &str,
    label: &str,
    unit: &str,
    value: &Value,
    resets_at: Option<DateTime<Utc>>,
) -> Option<QuotaWindow> {
    let snapshot = value.as_object()?;
    let entitlement = number(snapshot.get("entitlement"));
    let remaining =
        number(snapshot.get("remaining")).or_else(|| number(snapshot.get("quota_remaining")));
    if bool_value(snapshot.get("unlimited")) == Some(true)
        || entitlement == Some(-1.0)
        || remaining == Some(-1.0)
        || entitlement == Some(0.0)
    {
        return None;
    }

    let percent_remaining = number(snapshot.get("percent_remaining"));
    let used_percent = percent_remaining
        .map(|value| 100.0 - value)
        .or_else(|| {
            entitlement
                .zip(remaining)
                .filter(|(entitlement, _)| *entitlement > 0.0)
                .map(|(entitlement, remaining)| 100.0 - (remaining / entitlement) * 100.0)
        })?
        .clamp(0.0, 100.0);

    let counts = entitlement.zip(remaining).and_then(|(limit, remaining)| {
        (limit > 0.0 && remaining >= 0.0).then(|| ((limit - remaining).max(0.0), limit))
    });
    let (format, used_value, limit_value, unit) = if let Some((used, limit)) = counts {
        (
            QuotaFormat::Count,
            Some(used),
            Some(limit),
            Some(unit.to_owned()),
        )
    } else {
        (QuotaFormat::Percent, None, None, None)
    };

    Some(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent,
        resets_at,
        period_seconds: MONTHLY_PERIOD_SECONDS,
        format,
        used_value,
        limit_value,
        unit,
        estimated: false,
        source_note: None,
    })
}

fn overage_metric(value: &Value) -> Option<ValueMetric> {
    let snapshot = value.as_object()?;
    if bool_value(snapshot.get("overage_permitted")) != Some(true) {
        return None;
    }
    let count = number(snapshot.get("overage_count"))
        .unwrap_or(0.0)
        .max(0.0);
    Some(ValueMetric {
        id: "extra".into(),
        label: "Extra Usage".into(),
        values: vec![MetricValue {
            number: count,
            kind: MetricValueKind::Count,
            label: Some("credits".into()),
            estimated: false,
        }],
        expiries_at: Vec::new(),
    })
}

fn legacy_quota(
    id: &str,
    label: &str,
    unit: &str,
    remaining: Option<&Value>,
    total: Option<&Value>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<QuotaWindow> {
    let total = number(total).filter(|value| *value > 0.0)?;
    let remaining = number(remaining).filter(|value| *value >= 0.0)?;
    let used = (total - remaining).max(0.0);
    Some(QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: (used / total * 100.0).clamp(0.0, 100.0),
        resets_at,
        period_seconds: MONTHLY_PERIOD_SECONDS,
        format: QuotaFormat::Count,
        used_value: Some(used),
        limit_value: Some(total),
        unit: Some(unit.into()),
        estimated: false,
        source_note: None,
    })
}

fn reset_date(value: &Value) -> Option<DateTime<Utc>> {
    let value = value.as_str()?.trim();
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.to_utc())
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|date| date.and_utc())
        })
}

fn plan_label(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .split(|character: char| {
                character == '_' || character == '-' || character.is_whitespace()
            })
            .filter(|word| !word.is_empty())
            .map(|word| {
                let lower = word.to_ascii_lowercase();
                let mut characters = lower.chars();
                characters
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
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

fn bool_value(value: Option<&Value>) -> Option<bool> {
    let value = value?;
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

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{json, Value};

    use super::{map_org_usage, map_usage, org_logins, MONTHLY_PERIOD_SECONDS};
    use crate::{
        models::{MetricValueKind, QuotaFormat},
        providers::copilot::CopilotError,
    };

    fn paid_body() -> Value {
        json!({
            "copilot_plan": "pro",
            "quota_reset_date": "2099-01-15T00:00:00Z",
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 300,
                    "remaining": 123,
                    "percent_remaining": 41
                },
                "chat": {
                    "entitlement": 1000,
                    "remaining": 950,
                    "percent_remaining": 95
                }
            }
        })
    }

    fn quota<'a>(
        mapped: &'a super::CopilotMappedUsage,
        id: &str,
    ) -> &'a crate::models::QuotaWindow {
        mapped.quotas.iter().find(|quota| quota.id == id).unwrap()
    }

    fn value(mapped: &super::CopilotMappedUsage, id: &str) -> f64 {
        mapped
            .value_metrics
            .iter()
            .find(|metric| metric.id == id)
            .unwrap()
            .values[0]
            .number
    }

    #[test]
    fn paid_credit_and_chat_pools_map_as_exact_monthly_counts() {
        let mapped = map_usage(&paid_body()).unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Pro"));
        let credits = quota(&mapped, "premium");
        assert_eq!(credits.used_percent, 59.0);
        assert_eq!(credits.used_value, Some(177.0));
        assert_eq!(credits.limit_value, Some(300.0));
        assert_eq!(credits.format, QuotaFormat::Count);
        assert_eq!(credits.unit.as_deref(), Some("credits"));
        assert_eq!(credits.period_seconds, MONTHLY_PERIOD_SECONDS);
        assert_eq!(
            credits.resets_at,
            Utc.with_ymd_and_hms(2099, 1, 15, 0, 0, 0).single()
        );

        let chat = quota(&mapped, "chat");
        assert_eq!(chat.used_value, Some(50.0));
        assert_eq!(chat.unit.as_deref(), Some("requests"));
        assert!(!chat.estimated);
    }

    #[test]
    fn unlimited_sentinels_and_zero_entitlements_are_suppressed() {
        let mapped = map_usage(&json!({
            "copilot_plan": "individual",
            "quota_snapshots": {
                "premium_interactions": {"entitlement": 0, "remaining": 0},
                "chat": {"unlimited": true, "entitlement": 100, "remaining": 99},
                "completions": {"entitlement": -1, "remaining": -1}
            },
            "limited_user_quotas": {"chat": 50, "completions": 200},
            "monthly_quotas": {"chat": 100, "completions": 400}
        }))
        .unwrap();

        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["chat", "completions"]
        );
        assert_eq!(quota(&mapped, "chat").used_value, Some(50.0));
    }

    #[test]
    fn legacy_free_tier_counts_and_date_only_resets_are_supported() {
        let mapped = map_usage(&json!({
            "copilot_plan": "individual",
            "limited_user_quotas": {"chat": "250", "completions": 2000},
            "monthly_quotas": {"chat": 500, "completions": 4000},
            "limited_user_reset_date": "2099-02-15"
        }))
        .unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Individual"));
        assert_eq!(quota(&mapped, "chat").used_percent, 50.0);
        assert_eq!(quota(&mapped, "completions").used_value, Some(2000.0));
        assert_eq!(
            quota(&mapped, "chat").resets_at,
            Utc.with_ymd_and_hms(2099, 2, 15, 0, 0, 0).single()
        );
    }

    #[test]
    fn current_free_snapshot_shape_does_not_trigger_org_billing() {
        let mapped = map_usage(&json!({
            "copilot_plan":"individual",
            "token_based_billing":true,
            "quota_reset_date":"2099-07-01",
            "quota_snapshots":{
                "chat":{
                    "entitlement":200,
                    "remaining":182,
                    "percent_remaining":91,
                    "token_based_billing":true
                },
                "completions":{
                    "entitlement":2000,
                    "remaining":1989,
                    "percent_remaining":99.4,
                    "token_based_billing":true
                },
                "premium_interactions":{
                    "entitlement":0,
                    "remaining":0,
                    "percent_remaining":0,
                    "token_based_billing":true
                }
            }
        }))
        .unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Individual"));
        assert_eq!(quota(&mapped, "chat").used_value, Some(18.0));
        assert_eq!(quota(&mapped, "chat").used_percent, 9.0);
        assert_eq!(quota(&mapped, "completions").used_value, Some(11.0));
        assert!(!mapped.is_org_managed_seat);
    }

    #[test]
    fn paid_snapshots_do_not_mix_in_legacy_free_tier_rows() {
        let mut body = paid_body();
        body["quota_snapshots"]["chat"] = json!({"entitlement":-1,"remaining":-1,"unlimited":true});
        body["quota_snapshots"]["completions"] = json!({"entitlement":-1,"remaining":-1});
        body["limited_user_quotas"] = json!({"chat":100,"completions":1000});
        body["monthly_quotas"] = json!({"chat":500,"completions":4000});

        let mapped = map_usage(&body).unwrap();

        assert_eq!(mapped.quotas.len(), 1);
        assert_eq!(mapped.quotas[0].id, "premium");
    }

    #[test]
    fn overage_is_exact_including_a_real_zero_and_requires_a_real_credit_pool() {
        let mut body = paid_body();
        body["quota_snapshots"]["premium_interactions"]["overage_permitted"] = json!(true);
        body["quota_snapshots"]["premium_interactions"]["overage_count"] = json!(0);
        let mapped = map_usage(&body).unwrap();
        assert_eq!(value(&mapped, "extra"), 0.0);
        let extra = mapped
            .value_metrics
            .iter()
            .find(|metric| metric.id == "extra")
            .unwrap();
        assert_eq!(extra.values[0].label.as_deref(), Some("credits"));
        assert!(!extra.values[0].estimated);

        let placeholder = map_usage(&json!({
            "copilot_plan": "business",
            "token_based_billing": true,
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 0,
                    "remaining": 0,
                    "overage_permitted": true,
                    "overage_count": 0
                }
            }
        }))
        .unwrap();
        assert!(placeholder.quotas.is_empty());
        assert!(placeholder.value_metrics.is_empty());
        assert!(placeholder.is_org_managed_seat);
    }

    #[test]
    fn partial_optional_buckets_do_not_hide_valid_usage() {
        let mapped = map_usage(&json!({
            "copilot_plan": "pro_plus",
            "quota_reset_date": "not-a-date",
            "quota_snapshots": {
                "premium_requests": {
                    "entitlement": 100,
                    "remaining": 75
                },
                "chat": {"entitlement": "broken"},
                "completions": null
            }
        }))
        .unwrap();

        assert_eq!(mapped.plan.as_deref(), Some("Pro Plus"));
        assert_eq!(mapped.quotas.len(), 1);
        assert_eq!(mapped.quotas[0].id, "premium");
        assert_eq!(mapped.quotas[0].resets_at, None);
    }

    #[test]
    fn percentage_only_snapshot_is_not_presented_as_an_exact_count() {
        let mapped = map_usage(&json!({
            "quota_snapshots": {
                "chat": {"percent_remaining": 87.5}
            }
        }))
        .unwrap();
        let chat = quota(&mapped, "chat");
        assert_eq!(chat.used_percent, 12.5);
        assert_eq!(chat.format, QuotaFormat::Percent);
        assert_eq!(chat.used_value, None);
        assert_eq!(chat.unit, None);
    }

    #[test]
    fn negative_overage_remaining_keeps_a_clamped_percentage_meter() {
        let mapped = map_usage(&json!({
            "copilot_plan": "pro",
            "quota_snapshots": {
                "premium_interactions": {
                    "entitlement": 100,
                    "remaining": -2
                }
            }
        }))
        .unwrap();
        let credits = quota(&mapped, "premium");
        assert_eq!(credits.used_percent, 100.0);
        assert_eq!(credits.format, QuotaFormat::Percent);
        assert_eq!(credits.used_value, None);
        assert_eq!(credits.limit_value, None);
    }

    #[test]
    fn empty_and_malformed_payloads_are_distinct_from_org_managed_empty_usage() {
        assert!(matches!(
            map_usage(&Value::Null),
            Err(CopilotError::InvalidResponse)
        ));
        assert!(matches!(
            map_usage(&json!({"copilot_plan":"pro"})),
            Err(CopilotError::QuotaUnavailable)
        ));

        let org = map_usage(&json!({
            "copilot_plan": "business",
            "token_based_billing": "true",
            "quota_snapshots": {"premium_interactions":{"entitlement":0,"remaining":0}}
        }))
        .unwrap();
        assert_eq!(org.plan.as_deref(), Some("Business"));
        assert!(org.is_org_managed_seat);
    }

    #[test]
    fn zero_or_malformed_legacy_limits_do_not_create_fake_percentages() {
        for body in [
            json!({
                "limited_user_quotas":{"chat":0},
                "monthly_quotas":{"chat":0}
            }),
            json!({
                "limited_user_quotas":{"chat":"bad"},
                "monthly_quotas":{"chat":100}
            }),
        ] {
            assert!(matches!(
                map_usage(&body),
                Err(CopilotError::QuotaUnavailable)
            ));
        }
    }

    #[test]
    fn organization_logins_are_trimmed_and_structurally_safe() {
        assert_eq!(
            org_logins(&json!([
                {"login":" acme "},
                {"login":"globex-2"},
                {"login":"path/name"},
                {"id":3}
            ])),
            ["acme", "globex-2"]
        );
        assert!(org_logins(&json!({"login":"not-an-array"})).is_empty());
    }

    #[test]
    fn organization_credit_items_sum_exact_counts_and_spend() {
        let metrics = map_org_usage(&json!({
            "usageItems": [
                {
                    "product":"Copilot",
                    "unitType":"ai-units",
                    "grossQuantity":100.5,
                    "netAmount":1.25
                },
                {
                    "product":" copilot ",
                    "unitType":"AI-CREDITS",
                    "grossQuantity":"50",
                    "netAmount":0.5
                },
                {
                    "product":"Copilot",
                    "unitType":"user-months",
                    "grossQuantity":10,
                    "netAmount":190
                }
            ]
        }))
        .unwrap();

        assert_eq!(metrics[0].id, "orgCredits");
        assert_eq!(metrics[0].values[0].number, 150.5);
        assert_eq!(metrics[0].values[0].kind, MetricValueKind::Count);
        assert_eq!(metrics[0].values[0].label.as_deref(), Some("credits"));
        assert_eq!(metrics[1].values[0].number, 1.75);
        assert_eq!(metrics[1].values[0].kind, MetricValueKind::Dollars);
        assert!(metrics
            .iter()
            .all(|metric| { metric.values.iter().all(|value| !value.estimated) }));
    }

    #[test]
    fn organization_summary_without_credit_items_is_optional() {
        assert!(map_org_usage(&json!({
            "usageItems":[
                {"product":"Actions","unitType":"minutes","grossQuantity":100},
                {"product":"Copilot","unitType":"user-months","grossQuantity":2}
            ]
        }))
        .is_none());
        assert!(map_org_usage(&json!({"organization":"acme"})).is_none());
        assert!(map_org_usage(&Value::Null).is_none());
    }
}
