use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, UsageHistory, ValueMetric},
    pricing::ModelPricing,
    providers::{cursor::csv::CursorCsvRow, daily_usage::DailyUsageAccumulator},
};

use super::CursorError;

const BILLING_PERIOD_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug)]
pub struct CursorMappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    pub value_metrics: Vec<ValueMetric>,
}

#[derive(Debug)]
pub struct PlanUsageFacts {
    enabled: bool,
    has_plan_usage: bool,
    limit: Option<f64>,
    total_percent_used: Option<f64>,
    spend_limit_type: Option<String>,
    pooled_limit: f64,
}

impl PlanUsageFacts {
    pub fn new(usage: &Value) -> Self {
        let plan_usage = usage.get("planUsage").and_then(Value::as_object);
        let spend = usage.get("spendLimitUsage").and_then(Value::as_object);
        Self {
            enabled: usage.get("enabled").and_then(Value::as_bool) != Some(false),
            has_plan_usage: plan_usage.is_some(),
            limit: plan_usage
                .and_then(|value| value.get("limit"))
                .and_then(number),
            total_percent_used: plan_usage
                .and_then(|value| value.get("totalPercentUsed"))
                .and_then(number),
            spend_limit_type: spend
                .and_then(|value| value.get("limitType"))
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase),
            pooled_limit: spend
                .and_then(|value| value.get("pooledLimit"))
                .and_then(number)
                .unwrap_or_default(),
        }
    }

    fn plan_usage_unusable(&self) -> bool {
        !self.has_plan_usage || self.limit.is_none()
    }

    fn team_by_shape(&self) -> bool {
        self.spend_limit_type.as_deref() == Some("team") || self.pooled_limit > 0.0
    }

    pub fn should_try_generic_request_fallback(&self) -> bool {
        self.enabled
            && self.has_plan_usage
            && self.limit.is_none()
            && self.total_percent_used.is_none()
    }
}

pub fn request_fallback(
    usage: &Value,
    plan_name: Option<&str>,
    plan_unavailable: bool,
) -> Option<&'static str> {
    let facts = PlanUsageFacts::new(usage);
    if !facts.enabled {
        return None;
    }
    let plan = plan_name.unwrap_or_default().trim().to_ascii_lowercase();
    if facts.plan_usage_unusable() && plan == "enterprise" {
        return Some("Enterprise usage data unavailable. Try again later.");
    }
    if facts.plan_usage_unusable() && plan == "team" {
        return Some("Team request-based usage data unavailable. Try again later.");
    }
    if facts.plan_usage_unusable()
        && facts.total_percent_used.is_none()
        && plan.is_empty()
        && plan_unavailable
    {
        return Some("Cursor request-based usage data unavailable. Try again later.");
    }
    if facts.team_by_shape() && facts.has_plan_usage && facts.limit.is_none() {
        return Some("Cursor request-based usage data unavailable. Try again later.");
    }
    None
}

pub fn map_live_usage(
    usage: &Value,
    plan_name: Option<&str>,
    credit_grants: Option<&Value>,
    stripe_balance_cents: f64,
) -> Result<CursorMappedUsage, CursorError> {
    let facts = PlanUsageFacts::new(usage);
    let plan_usage = usage.get("planUsage").and_then(Value::as_object);
    if !facts.enabled || plan_usage.is_none() {
        return Err(CursorError::NoActiveSubscription);
    }
    if facts.limit.is_none() && facts.total_percent_used.is_none() {
        return Err(CursorError::TotalUsageLimitMissing);
    }
    let plan_usage = plan_usage.expect("checked above");
    let used_cents = plan_usage
        .get("totalSpend")
        .and_then(number)
        .unwrap_or_else(|| {
            facts.limit.unwrap_or_default()
                - plan_usage
                    .get("remaining")
                    .and_then(number)
                    .unwrap_or_default()
        })
        .max(0.0);
    let computed_percent = facts
        .limit
        .filter(|limit| *limit > 0.0)
        .map(|limit| used_cents / limit * 100.0)
        .unwrap_or_default();
    let total_percent = facts.total_percent_used.unwrap_or(computed_percent);
    let (resets_at, period_seconds) = billing_cycle(usage);
    let plan = plan_name.unwrap_or_default().trim().to_ascii_lowercase();
    let team = plan == "team" || facts.team_by_shape();
    let mut quotas = Vec::new();
    if team {
        let limit = facts.limit.ok_or_else(|| {
            CursorError::RequestBasedUnavailable(
                "Cursor request-based usage data unavailable. Try again later.".into(),
            )
        })?;
        quotas.push(quota(
            "usage",
            "Total usage",
            used_cents / 100.0,
            limit / 100.0,
            QuotaFormat::Dollars,
            resets_at,
            period_seconds,
        ));
    } else {
        quotas.push(percent_quota(
            "usage",
            "Total usage",
            total_percent,
            resets_at,
            period_seconds,
        ));
    }
    if let Some(used) = plan_usage.get("autoPercentUsed").and_then(number) {
        quotas.push(percent_quota(
            "auto",
            "Auto usage",
            used,
            resets_at,
            period_seconds,
        ));
    }
    if let Some(used) = plan_usage.get("apiPercentUsed").and_then(number) {
        quotas.push(percent_quota(
            "api",
            "API usage",
            used,
            resets_at,
            period_seconds,
        ));
    }

    let mut value_metrics = Vec::new();
    if let Some(spend) = usage.get("spendLimitUsage").and_then(Value::as_object) {
        let limit = spend
            .get("individualLimit")
            .and_then(number)
            .or_else(|| spend.get("pooledLimit").and_then(number))
            .unwrap_or_default();
        let remaining = spend
            .get("individualRemaining")
            .and_then(number)
            .or_else(|| spend.get("pooledRemaining").and_then(number))
            .unwrap_or_default();
        let spent = on_demand_spent(spend, limit, remaining);
        if limit > 0.0 {
            quotas.push(quota(
                "onDemand",
                "On-demand",
                spent / 100.0,
                limit / 100.0,
                QuotaFormat::Dollars,
                None,
                BILLING_PERIOD_SECONDS,
            ));
        } else if spent > 0.0 {
            value_metrics.push(dollar_value("onDemand", "On-demand", spent / 100.0));
        }
    }
    if let Some(remaining) = credits_remaining(credit_grants, stripe_balance_cents) {
        value_metrics.push(dollar_value("credits", "Credits", remaining / 100.0));
    }

    Ok(CursorMappedUsage {
        plan: plan_label(plan_name),
        quotas,
        value_metrics,
    })
}

pub fn map_request_usage(
    usage: &Value,
    plan_name: Option<&str>,
    unavailable_message: &str,
) -> Result<CursorMappedUsage, CursorError> {
    let gpt4 = usage.get("gpt-4").and_then(Value::as_object);
    let limit = gpt4
        .and_then(|value| value.get("maxRequestUsage"))
        .and_then(number)
        .filter(|limit| *limit > 0.0)
        .ok_or_else(|| CursorError::RequestBasedUnavailable(unavailable_message.into()))?;
    let used = gpt4
        .and_then(|value| value.get("numRequests"))
        .and_then(number)
        .unwrap_or_default();
    let resets_at = usage
        .get("startOfMonth")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.to_utc() + chrono::Duration::seconds(BILLING_PERIOD_SECONDS as i64));
    Ok(CursorMappedUsage {
        plan: plan_label(plan_name),
        quotas: vec![quota(
            "requests",
            "Requests",
            used,
            limit,
            QuotaFormat::Count,
            resets_at,
            BILLING_PERIOD_SECONDS,
        )],
        value_metrics: Vec::new(),
    })
}

pub fn stripe_balance_cents(response: Option<&Value>) -> f64 {
    response
        .and_then(|value| value.get("customerBalance"))
        .and_then(number)
        .filter(|balance| *balance < 0.0)
        .map(f64::abs)
        .unwrap_or_default()
}

pub fn usage_history(
    rows: &[CursorCsvRow],
    now: DateTime<Utc>,
    pricing: &ModelPricing,
) -> UsageHistory {
    let mut accumulator = DailyUsageAccumulator::default();
    for row in rows {
        let date = row.date.with_timezone(&chrono::Local).date_naive();
        let tokens = row.tokens.total_tokens();
        match row.estimated_cost_usd {
            Some(cost) => {
                let family = if row.model.trim().is_empty() {
                    "Unattributed".to_owned()
                } else {
                    pricing.display_family(row.model.trim())
                };
                accumulator.add_variant(date, tokens, cost, &family, row.model.trim());
            }
            None if tokens > 0 => accumulator.add_unknown_model(date, &row.model),
            None => {}
        }
    }
    accumulator.build(now, "From your Cursor usage export")
}

fn quota(
    id: &str,
    label: &str,
    used: f64,
    limit: f64,
    format: QuotaFormat,
    resets_at: Option<DateTime<Utc>>,
    period_seconds: u64,
) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: if limit > 0.0 {
            (used / limit * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        resets_at,
        period_seconds,
        format,
        used_value: Some(used),
        limit_value: Some(limit),
    }
}

fn percent_quota(
    id: &str,
    label: &str,
    used: f64,
    resets_at: Option<DateTime<Utc>>,
    period_seconds: u64,
) -> QuotaWindow {
    QuotaWindow {
        id: id.into(),
        label: label.into(),
        used_percent: used.clamp(0.0, 100.0),
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
    }
}

fn dollar_value(id: &str, label: &str, amount: f64) -> ValueMetric {
    ValueMetric {
        id: id.into(),
        label: label.into(),
        values: vec![MetricValue {
            number: amount,
            kind: MetricValueKind::Dollars,
            label: None,
        }],
        expiries_at: Vec::new(),
    }
}

fn on_demand_spent(spend: &serde_json::Map<String, Value>, limit: f64, remaining: f64) -> f64 {
    let reported = ["individualUsed", "pooledUsed", "totalSpend"]
        .iter()
        .filter_map(|key| spend.get(*key).and_then(number))
        .collect::<Vec<_>>();
    if let Some(positive) = reported.iter().copied().find(|value| *value > 0.0) {
        return positive;
    }
    let inferred = (limit - remaining).max(0.0);
    if inferred > 0.0 {
        inferred
    } else {
        reported.first().copied().unwrap_or_default()
    }
}

fn credits_remaining(credit_grants: Option<&Value>, stripe: f64) -> Option<f64> {
    let has_grants = credit_grants
        .and_then(|value| value.get("hasCreditGrants"))
        .and_then(Value::as_bool)
        == Some(true);
    let grant_total = if has_grants {
        credit_grants
            .and_then(|value| value.get("totalCents"))
            .and_then(number)
            .unwrap_or_default()
    } else {
        0.0
    };
    let valid_grants = has_grants && grant_total > 0.0;
    let grant_used = if valid_grants {
        credit_grants
            .and_then(|value| value.get("usedCents"))
            .and_then(number)
            .unwrap_or_default()
    } else {
        0.0
    };
    let total = if valid_grants { grant_total } else { 0.0 } + stripe;
    (total > 0.0).then(|| (total - grant_used).max(0.0))
}

fn billing_cycle(usage: &Value) -> (Option<DateTime<Utc>>, u64) {
    let start = usage.get("billingCycleStart").and_then(number);
    let end = usage.get("billingCycleEnd").and_then(number);
    let resets_at = end.and_then(timestamp_millis);
    let duration = start
        .zip(end)
        .filter(|(start, end)| end > start)
        .map(|(start, end)| ((end - start) / 1000.0) as u64)
        .unwrap_or(BILLING_PERIOD_SECONDS);
    (resets_at, duration)
}

fn timestamp_millis(value: f64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value as i64)
}

fn plan_label(plan: Option<&str>) -> Option<String> {
    let plan = plan?.trim();
    if plan.is_empty() {
        return None;
    }
    Some(
        plan.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::TimeZone;
    use serde_json::json;

    use crate::pricing::{PricingCatalog, PricingSupplement, TokenBreakdown};

    use super::*;

    #[test]
    fn live_mapper_preserves_positive_spend_and_combines_credits() {
        let usage = json!({
            "enabled": true,
            "billingCycleStart": 1_700_000_000_000_f64,
            "billingCycleEnd": 1_702_592_000_000_f64,
            "planUsage": {"limit": 40000, "remaining": 32000, "totalPercentUsed": 20,
                          "autoPercentUsed": 12, "apiPercentUsed": 4},
            "spendLimitUsage": {"individualLimit": 10000, "individualRemaining": 10000,
                                "individualUsed": 0, "pooledUsed": 2500}
        });
        let mapped = map_live_usage(
            &usage,
            Some("pro plan"),
            Some(&json!({"hasCreditGrants": true, "totalCents": 2000, "usedCents": 500})),
            1000.0,
        )
        .unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Pro Plan"));
        assert_eq!(
            mapped
                .quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["usage", "auto", "api", "onDemand"]
        );
        assert_eq!(mapped.quotas.last().unwrap().used_value, Some(25.0));
        assert_eq!(mapped.value_metrics[0].values[0].number, 25.0);
    }

    #[test]
    fn team_total_is_dollars_and_unbounded_on_demand_is_a_value() {
        let usage = json!({
            "enabled": true,
            "planUsage": {"limit": 50000, "totalSpend": 12500},
            "spendLimitUsage": {"limitType": "team", "totalSpend": 4200}
        });
        let mapped = map_live_usage(&usage, Some("team"), None, 0.0).unwrap();
        assert_eq!(mapped.quotas[0].format, QuotaFormat::Dollars);
        assert_eq!(mapped.quotas[0].used_value, Some(125.0));
        assert_eq!(mapped.value_metrics[0].id, "onDemand");
        assert_eq!(mapped.value_metrics[0].values[0].number, 42.0);
    }

    #[test]
    fn request_fallback_maps_counts_and_month_reset() {
        let mapped = map_request_usage(
            &json!({"gpt-4":{"numRequests":120,"maxRequestUsage":500},"startOfMonth":"2026-07-01T00:00:00Z"}),
            Some("enterprise"),
            "unavailable",
        )
        .unwrap();
        assert_eq!(mapped.quotas[0].format, QuotaFormat::Count);
        assert_eq!(mapped.quotas[0].used_value, Some(120.0));
        assert_eq!(mapped.quotas[0].limit_value, Some(500.0));
    }

    #[test]
    fn fallback_predicates_cover_unknown_plan_and_team_shape() {
        let unusable = json!({"enabled":true,"planUsage":{}});
        assert!(request_fallback(&unusable, None, true).is_some());
        let team = json!({"enabled":true,"planUsage":{},"spendLimitUsage":{"pooledLimit":100}});
        assert!(request_fallback(&team, Some("business"), false).is_some());
        assert!(PlanUsageFacts::new(&unusable).should_try_generic_request_fallback());
    }

    #[test]
    fn csv_history_groups_cursor_variants_and_scopes_unknown_models() {
        let supplement = PricingSupplement::decode(
            br#"{
              "pricing":{"claude-opus-4-8":{"input_per_million":1,"output_per_million":2}},
              "alias_rules":[
                {"pattern":"^claude-opus-4-8-thinking-(?:max|high)$","canonical":"claude-opus-4-8"}
              ]
            }"#,
        )
        .unwrap();
        let pricing = ModelPricing::new(
            supplement,
            PricingCatalog {
                entries: HashMap::new(),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        );
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();
        let rows = vec![
            CursorCsvRow {
                date: now,
                model: "claude-opus-4-8-thinking-max".into(),
                tokens: TokenBreakdown {
                    input: 300,
                    ..TokenBreakdown::default()
                },
                estimated_cost_usd: Some(3.004),
            },
            CursorCsvRow {
                date: now,
                model: "claude-opus-4-8-thinking-high".into(),
                tokens: TokenBreakdown {
                    input: 100,
                    ..TokenBreakdown::default()
                },
                estimated_cost_usd: Some(1.006),
            },
            CursorCsvRow {
                date: now,
                model: "unknown-cursor-model".into(),
                tokens: TokenBreakdown {
                    input: 50,
                    ..TokenBreakdown::default()
                },
                estimated_cost_usd: None,
            },
        ];

        let history = usage_history(&rows, now, &pricing);
        let today = history.today.unwrap();
        assert_eq!(
            today.tokens, 400,
            "unpriced tokens stay out of coherent totals"
        );
        assert_eq!(today.estimated_cost_usd, Some(4.01));
        assert_eq!(today.unknown_models, ["unknown-cursor-model"]);
        let models = today.model_breakdown.unwrap().models;
        assert_eq!(models[0].model, "claude-opus-4-8");
        assert_eq!(
            models[0]
                .variants
                .as_ref()
                .unwrap()
                .iter()
                .map(|variant| variant.model.as_str())
                .collect::<Vec<_>>(),
            [
                "claude-opus-4-8-thinking-max",
                "claude-opus-4-8-thinking-high"
            ]
        );
    }
}
