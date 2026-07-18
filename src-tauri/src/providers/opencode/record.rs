use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::pricing::{ModelPricing, TokenBreakdown};

pub(crate) const GO_PROVIDER_ID: &str = "opencode-go";
pub(super) const EPOCH_MILLISECONDS_THRESHOLD: i64 = 100_000_000_000;
const HOSTED_PROVIDER_IDS: [&str; 2] = [GO_PROVIDER_ID, "opencode"];
const MAX_TOKEN_VALUE: f64 = 1_000_000_000_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostProvenance {
    Exact,
    Estimated,
}

#[derive(Debug, Clone)]
pub(crate) struct UsageRecord {
    pub(crate) key: (String, String),
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) tokens: u64,
    pub(crate) cost: Option<f64>,
    pub(crate) cost_provenance: CostProvenance,
    pub(crate) incomplete_cost: bool,
}

#[derive(Debug)]
pub(super) struct ParsedMessage {
    pub(super) message_id: String,
    session_id: String,
    pub(super) timestamp: DateTime<Utc>,
    provider_id: String,
    model: String,
    tokens: Option<ParsedTokens>,
    cost: StoredCost,
}

impl ParsedMessage {
    pub(super) fn into_usage(self, parts: &[ParsedPart], pricing: &ModelPricing) -> UsageRecord {
        let part_tokens = parts
            .iter()
            .filter_map(|part| part.tokens)
            .fold(ParsedTokens::default(), ParsedTokens::saturating_add);
        let tokens = self
            .tokens
            .filter(|tokens| tokens.total > 0 || part_tokens.total == 0)
            .unwrap_or(part_tokens);

        let (cost, cost_provenance, incomplete_cost) = match self.cost {
            StoredCost::Exact(cost) => (Some(cost), CostProvenance::Exact, false),
            StoredCost::Invalid | StoredCost::Missing if !parts.is_empty() => {
                cost_from_parts(parts, &self.provider_id, &self.model, pricing)
            }
            StoredCost::Invalid => (None, CostProvenance::Exact, tokens.total > 0),
            StoredCost::Missing => {
                estimate_cost(pricing, &self.provider_id, &self.model, tokens.breakdown).map_or(
                    (None, CostProvenance::Estimated, tokens.total > 0),
                    |cost| (Some(cost), CostProvenance::Estimated, false),
                )
            }
        };

        UsageRecord {
            key: (self.session_id, self.message_id),
            timestamp: self.timestamp,
            provider_id: self.provider_id,
            model: self.model,
            tokens: tokens.total,
            cost,
            cost_provenance,
            incomplete_cost,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ParsedTokens {
    breakdown: TokenBreakdown,
    total: u64,
}

impl ParsedTokens {
    fn saturating_add(self, other: Self) -> Self {
        Self {
            breakdown: TokenBreakdown {
                input: self.breakdown.input.saturating_add(other.breakdown.input),
                cache_write_5m: self
                    .breakdown
                    .cache_write_5m
                    .saturating_add(other.breakdown.cache_write_5m),
                cache_write_1h: self
                    .breakdown
                    .cache_write_1h
                    .saturating_add(other.breakdown.cache_write_1h),
                cache_read: self
                    .breakdown
                    .cache_read
                    .saturating_add(other.breakdown.cache_read),
                output: self.breakdown.output.saturating_add(other.breakdown.output),
                is_fast: self.breakdown.is_fast || other.breakdown.is_fast,
            },
            total: self.total.saturating_add(other.total),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StoredCost {
    Missing,
    Invalid,
    Exact(f64),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ParsedPart {
    tokens: Option<ParsedTokens>,
    cost: StoredCost,
}

pub(super) fn parse_message(
    session_id: String,
    message_id: String,
    column_timestamp: Option<i64>,
    value: &Value,
) -> Option<ParsedMessage> {
    if value.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let provider_id = provider_id(value)?;
    if !HOSTED_PROVIDER_IDS.contains(&provider_id.as_str()) {
        return None;
    }
    let timestamp = column_timestamp
        .and_then(timestamp_from_number)
        .or_else(|| {
            value
                .pointer("/time/created")
                .and_then(timestamp_from_value)
        })
        .or_else(|| value.get("time_created").and_then(timestamp_from_value))?;
    let model = model_id(value).unwrap_or_else(|| "Unattributed".into());
    Some(ParsedMessage {
        session_id,
        message_id,
        timestamp,
        provider_id,
        model,
        tokens: value.get("tokens").and_then(parse_tokens),
        cost: stored_cost(value, &["cost", "costUSD"]),
    })
}

pub(super) fn parse_part(value: &Value) -> Option<ParsedPart> {
    if !matches!(
        value.get("type").and_then(Value::as_str),
        Some("step-finish" | "step_finish")
    ) {
        return None;
    }
    Some(ParsedPart {
        tokens: value.get("tokens").and_then(parse_tokens),
        cost: stored_cost(value, &["cost", "costUSD"]),
    })
}

pub(super) fn provider_id(value: &Value) -> Option<String> {
    text_at(
        value,
        &[
            "/providerID",
            "/providerId",
            "/provider_id",
            "/model/providerID",
            "/model/providerId",
        ],
    )
}

pub(super) fn timestamp_from_value(value: &Value) -> Option<DateTime<Utc>> {
    finite_number(value)
        .and_then(|number| timestamp_from_number(number.trunc() as i64))
        .or_else(|| {
            value
                .as_str()
                .and_then(super::super::log_usage::parse_log_timestamp)
        })
}

pub(super) fn timestamp_from_number(value: i64) -> Option<DateTime<Utc>> {
    let milliseconds = if value.unsigned_abs() < EPOCH_MILLISECONDS_THRESHOLD as u64 {
        value.saturating_mul(1_000)
    } else {
        value
    };
    DateTime::from_timestamp_millis(milliseconds)
}

fn cost_from_parts(
    parts: &[ParsedPart],
    provider_id: &str,
    model: &str,
    pricing: &ModelPricing,
) -> (Option<f64>, CostProvenance, bool) {
    let mut total = 0.0;
    let mut has_cost = false;
    let mut estimated = false;
    let mut incomplete = false;
    for part in parts {
        match part.cost {
            StoredCost::Exact(cost) => {
                total += cost;
                has_cost = true;
            }
            StoredCost::Missing => {
                let estimate = part.tokens.and_then(|tokens| {
                    estimate_cost(pricing, provider_id, model, tokens.breakdown)
                });
                if let Some(cost) = estimate {
                    total += cost;
                    has_cost = true;
                    estimated = true;
                } else if part.tokens.is_some_and(|tokens| tokens.total > 0) {
                    incomplete = true;
                }
            }
            StoredCost::Invalid => incomplete = true,
        }
    }
    (
        has_cost.then_some(total),
        if estimated {
            CostProvenance::Estimated
        } else {
            CostProvenance::Exact
        },
        incomplete,
    )
}

fn estimate_cost(
    pricing: &ModelPricing,
    provider_id: &str,
    model: &str,
    tokens: TokenBreakdown,
) -> Option<f64> {
    if tokens.total_tokens() == 0 || model == "Unattributed" {
        return None;
    }
    pricing
        .estimated_cost_dollars(model, tokens, true)
        .or_else(|| pricing.estimated_cost_dollars(&format!("{provider_id}/{model}"), tokens, true))
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
}

fn model_id(value: &Value) -> Option<String> {
    text_at(
        value,
        &[
            "/modelID",
            "/modelId",
            "/model_id",
            "/model/modelID",
            "/model/modelId",
            "/model/id",
        ],
    )
}

fn text_at(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .and_then(non_empty)
    })
}

fn parse_tokens(value: &Value) -> Option<ParsedTokens> {
    let object = value.as_object()?;
    let input = integer_at(object, &["input"]);
    let output = integer_at(object, &["output"]);
    let reasoning = integer_at(object, &["reasoning"]);
    let cache_read = value
        .pointer("/cache/read")
        .and_then(nonnegative_integer)
        .unwrap_or_default();
    let cache_write = value
        .pointer("/cache/write")
        .and_then(nonnegative_integer)
        .unwrap_or_default();
    let recomputed = input
        .saturating_add(output)
        .saturating_add(reasoning)
        .saturating_add(cache_read)
        .saturating_add(cache_write);
    let reported = object.get("total").and_then(nonnegative_integer);
    Some(ParsedTokens {
        breakdown: TokenBreakdown {
            input,
            cache_write_5m: cache_write,
            cache_read,
            output: output.saturating_add(reasoning),
            ..TokenBreakdown::default()
        },
        total: reported.filter(|total| *total > 0).unwrap_or(recomputed),
    })
}

fn integer_at(object: &Map<String, Value>, names: &[&str]) -> u64 {
    names
        .iter()
        .find_map(|name| object.get(*name).and_then(nonnegative_integer))
        .unwrap_or_default()
}

fn nonnegative_integer(value: &Value) -> Option<u64> {
    finite_number(value)
        .filter(|value| *value >= 0.0)
        .map(|value| value.min(MAX_TOKEN_VALUE).trunc() as u64)
}

fn stored_cost(value: &Value, names: &[&str]) -> StoredCost {
    let Some(value) = names.iter().find_map(|name| value.get(*name)) else {
        return StoredCost::Missing;
    };
    match finite_number(value) {
        Some(cost) if cost >= 0.0 => StoredCost::Exact(cost),
        _ => StoredCost::Invalid,
    }
}

fn finite_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value| value.is_finite())
}

fn non_empty(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| value.to_owned())
}
