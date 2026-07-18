use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::models::{MetricValue, MetricValueKind, QuotaFormat, QuotaWindow, ValueMetric};

use super::{client::UsageResponse, CodexError};

const SESSION_PERIOD_SECONDS: u64 = 5 * 60 * 60;
const WEEKLY_PERIOD_SECONDS: u64 = 7 * 24 * 60 * 60;
const CREDIT_USD_RATE: f64 = 0.04;

pub struct MappedUsage {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaWindow>,
    pub value_metrics: Vec<ValueMetric>,
}

pub fn map_usage(
    response: &UsageResponse,
    reset_credits: Option<&UsageResponse>,
    now: DateTime<Utc>,
) -> Result<MappedUsage, CodexError> {
    if matches!(response.status.as_u16(), 401 | 403) {
        return Err(CodexError::TokenExpired);
    }
    if !response.status.is_success() {
        return Err(CodexError::RequestFailed(response.status.as_u16()));
    }
    if !response.body.is_object() {
        return Err(CodexError::InvalidResponse);
    }

    let rate_limit = response.body.get("rate_limit");
    let mut quotas = map_classified_windows(
        rate_limit,
        WindowMetricLabels {
            session_id: "session",
            session_label: "Session",
            weekly_id: "weekly",
            weekly_label: "Weekly",
        },
        header_number(response, "x-codex-primary-used-percent"),
        header_number(response, "x-codex-secondary-used-percent"),
        now,
    );
    quotas.extend(map_spark_windows(&response.body, now));

    let mut value_metrics = Vec::new();
    if let Some(metric) = map_reset_credits(&response.body, reset_credits) {
        value_metrics.push(metric);
    }
    if let Some(balance) = read_credits_remaining(response) {
        value_metrics.push(credits_metric(balance));
    }

    Ok(MappedUsage {
        plan: format_plan(response.body.get("plan_type")),
        quotas,
        value_metrics,
    })
}

fn map_spark_windows(body: &Value, now: DateTime<Utc>) -> Vec<QuotaWindow> {
    let Some(entry) = body
        .get("additional_rate_limits")
        .and_then(Value::as_array)
        .and_then(|entries| entries.iter().find(|entry| is_spark_entry(entry)))
    else {
        return Vec::new();
    };
    let rate_limit = entry.get("rate_limit");
    map_classified_windows(
        rate_limit,
        WindowMetricLabels {
            session_id: "spark",
            session_label: "Spark",
            weekly_id: "sparkWeekly",
            weekly_label: "Spark Weekly",
        },
        None,
        None,
        now,
    )
}

fn is_spark_entry(entry: &Value) -> bool {
    ["limit_name", "metered_feature"].into_iter().any(|key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value.to_ascii_lowercase().contains("spark"))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    Session,
    Weekly,
}

struct WindowMetricLabels<'a> {
    session_id: &'a str,
    session_label: &'a str,
    weekly_id: &'a str,
    weekly_label: &'a str,
}

#[derive(Clone, Copy)]
struct WindowCandidate<'a> {
    window: Option<&'a Value>,
    used_percent: Option<f64>,
    fallback_kind: WindowKind,
}

fn map_classified_windows(
    rate_limit: Option<&Value>,
    labels: WindowMetricLabels<'_>,
    primary_header: Option<f64>,
    secondary_header: Option<f64>,
    now: DateTime<Utc>,
) -> Vec<QuotaWindow> {
    let candidates = [
        window_candidate(
            rate_limit.and_then(|value| value.get("primary_window")),
            primary_header,
            WindowKind::Session,
        ),
        window_candidate(
            rate_limit.and_then(|value| value.get("secondary_window")),
            secondary_header,
            WindowKind::Weekly,
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    [
        map_classified_window(
            WindowKind::Session,
            labels.session_id,
            labels.session_label,
            &candidates,
            now,
        ),
        map_classified_window(
            WindowKind::Weekly,
            labels.weekly_id,
            labels.weekly_label,
            &candidates,
            now,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn window_candidate<'a>(
    value: Option<&'a Value>,
    header_fallback: Option<f64>,
    fallback_kind: WindowKind,
) -> Option<WindowCandidate<'a>> {
    let window = value.filter(|value| value.is_object());
    if window.is_none() && header_fallback.is_none() {
        return None;
    }
    Some(WindowCandidate {
        window,
        used_percent: window
            .and_then(|window| number(window.get("used_percent")))
            .or(header_fallback),
        fallback_kind,
    })
}

fn map_classified_window(
    kind: WindowKind,
    id: &str,
    label: &str,
    candidates: &[WindowCandidate<'_>],
    now: DateTime<Utc>,
) -> Option<QuotaWindow> {
    // The service normally uses primary for five-hour limits and secondary for weekly
    // limits, but a temporarily sole weekly limit can appear in the primary slot.
    let candidate = candidates
        .iter()
        .find(|candidate| exact_kind(candidate.window) == Some(kind))
        .or_else(|| {
            candidates.iter().find(|candidate| {
                exact_kind(candidate.window).is_none() && candidate.fallback_kind == kind
            })
        })?;
    let default_period = match kind {
        WindowKind::Session => SESSION_PERIOD_SECONDS,
        WindowKind::Weekly => WEEKLY_PERIOD_SECONDS,
    };
    map_window(id, label, *candidate, default_period, now)
}

fn exact_kind(window: Option<&Value>) -> Option<WindowKind> {
    match window.and_then(|window| number(window.get("limit_window_seconds"))) {
        Some(seconds) if seconds == SESSION_PERIOD_SECONDS as f64 => Some(WindowKind::Session),
        Some(seconds) if seconds == WEEKLY_PERIOD_SECONDS as f64 => Some(WindowKind::Weekly),
        _ => None,
    }
}

fn map_window(
    id: &str,
    label: &str,
    candidate: WindowCandidate<'_>,
    default_period: u64,
    now: DateTime<Utc>,
) -> Option<QuotaWindow> {
    let used_percent = candidate.used_percent?;
    let resets_at = candidate.window.and_then(|window| {
        number(window.get("reset_at"))
            .and_then(timestamp)
            .or_else(|| {
                number(window.get("reset_after_seconds"))
                    .map(|seconds| now + Duration::milliseconds((seconds * 1000.0) as i64))
            })
    });
    let period_seconds = candidate
        .window
        .and_then(|window| number(window.get("limit_window_seconds")))
        .map(|value| value.max(0.0) as u64)
        .unwrap_or(default_period);
    Some(QuotaWindow {
        id: id.to_owned(),
        label: label.to_owned(),
        used_percent,
        resets_at,
        period_seconds,
        format: QuotaFormat::Percent,
        used_value: None,
        limit_value: None,
        unit: None,
        estimated: false,
        source_note: None,
    })
}

fn map_reset_credits(body: &Value, dedicated: Option<&UsageResponse>) -> Option<ValueMetric> {
    let source = reset_credits_source(body, dedicated)?;
    let count = number(source.get("available_count"))?;
    if count < 0.0 {
        return None;
    }
    let mut expiries_at = source
        .get("credits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|credit| {
            credit
                .get("status")
                .and_then(Value::as_str)
                .is_none_or(|status| status == "available")
        })
        .filter_map(|credit| parse_expiry(credit.get("expires_at")))
        .collect::<Vec<_>>();
    expiries_at.sort();
    Some(ValueMetric {
        id: "rateLimitResets".into(),
        label: "Rate Limit Resets".into(),
        values: vec![MetricValue {
            number: count.floor(),
            kind: MetricValueKind::Count,
            label: Some("available".into()),
            estimated: false,
        }],
        expiries_at,
    })
}

fn reset_credits_source<'a>(
    body: &'a Value,
    dedicated: Option<&'a UsageResponse>,
) -> Option<&'a Value> {
    if let Some(response) = dedicated.filter(|response| response.status.is_success()) {
        if response.body.is_object() && number(response.body.get("available_count")).is_some() {
            return Some(&response.body);
        }
    }
    body.get("rate_limit_reset_credits")
        .filter(|value| value.is_object())
}

fn credits_metric(remaining: f64) -> ValueMetric {
    let credits = remaining.max(0.0).floor();
    ValueMetric {
        id: "credits".into(),
        label: "Extra Usage".into(),
        values: vec![
            MetricValue {
                number: credits * CREDIT_USD_RATE,
                kind: MetricValueKind::Dollars,
                label: None,
                estimated: true,
            },
            MetricValue {
                number: credits,
                kind: MetricValueKind::Count,
                label: Some("credits".into()),
                estimated: false,
            },
        ],
        expiries_at: Vec::new(),
    }
}

fn read_credits_remaining(response: &UsageResponse) -> Option<f64> {
    if let Some(credits) = response.body.get("credits") {
        if let Some(balance) = number(credits.get("balance")) {
            return Some(balance);
        }
        if credits.get("has_credits").and_then(Value::as_bool) == Some(false) {
            return Some(0.0);
        }
    }
    header_number(response, "x-codex-credits-balance")
}

fn parse_expiry(value: Option<&Value>) -> Option<DateTime<Utc>> {
    if let Some(value) = value.and_then(Value::as_str) {
        return DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|date| date.to_utc());
    }
    number(value).and_then(timestamp)
}

fn timestamp(seconds: f64) -> Option<DateTime<Utc>> {
    if !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let nanos = ((seconds - seconds.floor()) * 1_000_000_000.0) as u32;
    DateTime::from_timestamp(whole, nanos)
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
            .filter(|number| number.is_finite())
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
    use serde_json::{json, Value};

    use super::{map_usage, SESSION_PERIOD_SECONDS, WEEKLY_PERIOD_SECONDS};
    use crate::{
        models::{
            AppSettings, MetricLayout, MetricSection, MetricValueKind, NotificationPreferences,
            ProviderLayout, ProviderSnapshot, UsageHistory, ValueMetric,
        },
        pacing::NotificationEvaluator,
        providers::{codex::client::UsageResponse, ProviderRegistry},
    };

    fn response(body: Value) -> UsageResponse {
        UsageResponse {
            status: StatusCode::OK,
            headers: HashMap::new(),
            body,
        }
    }

    fn value_metric<'a>(mapped: &'a super::MappedUsage, id: &str) -> &'a ValueMetric {
        mapped
            .value_metrics
            .iter()
            .find(|metric| metric.id == id)
            .unwrap()
    }

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
        let mapped = map_usage(&response, None, now).unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Pro 5x"));
        assert_eq!(mapped.quotas[0].used_percent, 10.0);
        assert_eq!(mapped.quotas[1].used_percent, 20.0);
        assert_eq!(mapped.quotas[0].period_seconds, SESSION_PERIOD_SECONDS);
    }

    #[test]
    fn maps_weekly_only_primary_window_by_duration() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mapped = map_usage(
            &response(json!({
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 5,
                        "limit_window_seconds": 604800,
                        "reset_after_seconds": 60
                    },
                    "secondary_window": null
                }
            })),
            None,
            now,
        )
        .unwrap();

        assert_eq!(mapped.quotas.len(), 1);
        assert_eq!(mapped.quotas[0].id, "weekly");
        assert_eq!(mapped.quotas[0].used_percent, 5.0);
        assert_eq!(mapped.quotas[0].period_seconds, WEEKLY_PERIOD_SECONDS);
    }

    #[test]
    fn weekly_only_mapping_cannot_reuse_a_stale_session_notification_baseline() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let registry =
            ProviderRegistry::from_definitions(vec![crate::providers::codex::definition()])
                .unwrap();
        let settings = AppSettings {
            providers: vec![ProviderLayout {
                id: "codex".into(),
                enabled: true,
                detected: true,
                expanded: false,
                metrics: ["codex.session", "codex.weekly"]
                    .into_iter()
                    .map(|id| MetricLayout {
                        id: id.into(),
                        enabled: true,
                        section: MetricSection::AlwaysVisible,
                        pinned: false,
                    })
                    .collect(),
            }],
            notifications: NotificationPreferences {
                almost_out: false,
                cutting_it_close: false,
                will_run_out: true,
            },
            ..AppSettings::default()
        };
        let snapshot = |duration: u64, used_percent: f64| {
            let mapped = map_usage(
                &response(json!({
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": used_percent,
                            "limit_window_seconds": duration,
                            "reset_after_seconds": duration / 2
                        },
                        "secondary_window": null
                    }
                })),
                None,
                now,
            )
            .unwrap();
            ProviderSnapshot {
                provider_id: "codex".into(),
                plan: mapped.plan,
                quotas: mapped.quotas,
                value_metrics: mapped.value_metrics,
                status_metrics: Vec::new(),
                notices: Vec::new(),
                usage: UsageHistory::default(),
                warnings: Vec::new(),
                refreshed_at: now,
            }
        };
        let evaluator = NotificationEvaluator::default();

        assert!(evaluator
            .evaluate(
                &snapshot(SESSION_PERIOD_SECONDS, 10.0),
                &settings,
                &registry,
                now,
            )
            .is_empty());
        assert!(evaluator
            .evaluate(
                &snapshot(WEEKLY_PERIOD_SECONDS, 60.0),
                &settings,
                &registry,
                now,
            )
            .is_empty());
        assert!(evaluator
            .evaluate(
                &snapshot(SESSION_PERIOD_SECONDS, 60.0),
                &settings,
                &registry,
                now,
            )
            .is_empty());
    }

    #[test]
    fn unfamiliar_window_durations_keep_positional_fallbacks() {
        let mapped = map_usage(
            &response(json!({
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 11,
                        "limit_window_seconds": 86400
                    },
                    "secondary_window": {
                        "used_percent": 22,
                        "limit_window_seconds": 2592000
                    }
                }
            })),
            None,
            Utc::now(),
        )
        .unwrap();

        assert_eq!(mapped.quotas[0].id, "session");
        assert_eq!(mapped.quotas[0].used_percent, 11.0);
        assert_eq!(mapped.quotas[1].id, "weekly");
        assert_eq!(mapped.quotas[1].used_percent, 22.0);
    }

    #[test]
    fn maps_spark_windows_and_matches_metered_feature() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mapped = map_usage(
            &response(json!({
                "additional_rate_limits": [{
                    "limit_name": "Research Preview",
                    "metered_feature": "codex_spark_preview",
                    "rate_limit": {
                        "primary_window": {"used_percent": 25, "limit_window_seconds": 18000},
                        "secondary_window": {"used_percent": 40, "limit_window_seconds": 604800}
                    }
                }]
            })),
            None,
            now,
        )
        .unwrap();
        assert_eq!(mapped.quotas[0].id, "spark");
        assert_eq!(mapped.quotas[0].used_percent, 25.0);
        assert_eq!(mapped.quotas[1].id, "sparkWeekly");
    }

    #[test]
    fn maps_weekly_only_spark_primary_window_by_duration() {
        let mapped = map_usage(
            &response(json!({
                "additional_rate_limits": [{
                    "limit_name": "GPT-Codex-Spark",
                    "rate_limit": {
                        "primary_window": {
                            "used_percent": 7,
                            "limit_window_seconds": 604800,
                            "reset_after_seconds": 60
                        },
                        "secondary_window": null
                    }
                }]
            })),
            None,
            Utc::now(),
        )
        .unwrap();

        assert_eq!(mapped.quotas.len(), 1);
        assert_eq!(mapped.quotas[0].id, "sparkWeekly");
        assert_eq!(mapped.quotas[0].used_percent, 7.0);
        assert_eq!(mapped.quotas[0].period_seconds, WEEKLY_PERIOD_SECONDS);
    }

    #[test]
    fn ignores_non_spark_and_malformed_additional_limits() {
        let mapped = map_usage(
            &response(json!({
                "additional_rate_limits": [
                    null,
                    {"limit_name": "Some Other Model", "rate_limit": {"primary_window": {"used_percent": 50}}},
                    {"limit_name": "GPT-Codex-Spark"}
                ]
            })),
            None,
            Utc::now(),
        )
        .unwrap();
        assert!(mapped.quotas.is_empty());
    }

    #[test]
    fn credits_floor_before_pricing_and_body_precedes_header() {
        let mut usage = response(json!({"credits": {"balance": 821.9}}));
        usage
            .headers
            .insert("x-codex-credits-balance".into(), "999".into());
        let mapped = map_usage(&usage, None, Utc::now()).unwrap();
        let credits = value_metric(&mapped, "credits");
        assert_eq!(credits.values[0].kind, MetricValueKind::Dollars);
        assert_eq!(credits.values[0].number, 32.84);
        assert_eq!(credits.values[1].number, 821.0);
    }

    #[test]
    fn credits_use_header_fallback_and_preserve_a_measured_zero() {
        let mut header = response(json!({}));
        header
            .headers
            .insert("x-codex-credits-balance".into(), "12.9".into());
        let mapped = map_usage(&header, None, Utc::now()).unwrap();
        assert_eq!(value_metric(&mapped, "credits").values[1].number, 12.0);

        let mapped = map_usage(
            &response(json!({"credits": {"has_credits": false}})),
            None,
            Utc::now(),
        )
        .unwrap();
        assert_eq!(value_metric(&mapped, "credits").values[1].number, 0.0);
    }

    #[test]
    fn dedicated_reset_credits_precede_credits_and_sort_available_expiries() {
        let usage = response(json!({
            "rate_limit_reset_credits": {"available_count": 9},
            "credits": {"balance": 100}
        }));
        let dedicated = response(json!({
            "available_count": 2,
            "credits": [
                {"status": "available", "expires_at": "2026-02-20T19:00:00Z"},
                {"expires_at": "2026-02-20T17:30:00Z"},
                {"status": "consumed", "expires_at": "2026-02-20T16:10:00Z"}
            ]
        }));
        let mapped = map_usage(&usage, Some(&dedicated), Utc::now()).unwrap();
        assert_eq!(mapped.value_metrics[0].id, "rateLimitResets");
        assert_eq!(mapped.value_metrics[0].values[0].number, 2.0);
        assert_eq!(mapped.value_metrics[0].expiries_at.len(), 2);
        assert!(mapped.value_metrics[0].expiries_at[0] < mapped.value_metrics[0].expiries_at[1]);
        assert_eq!(mapped.value_metrics[1].id, "credits");
    }

    #[test]
    fn unusable_dedicated_count_falls_back_and_zero_is_preserved() {
        let usage = response(json!({"rate_limit_reset_credits": {"available_count": 0}}));
        let dedicated = response(json!({"available_count": null}));
        let mapped = map_usage(&usage, Some(&dedicated), Utc::now()).unwrap();
        assert_eq!(
            value_metric(&mapped, "rateLimitResets").values[0].number,
            0.0
        );
    }

    #[test]
    fn non_success_dedicated_response_falls_back_to_usage_count() {
        let usage = response(json!({"rate_limit_reset_credits": {"available_count": 3}}));
        let mut dedicated = response(json!({"available_count": 9}));
        dedicated.status = StatusCode::INTERNAL_SERVER_ERROR;
        let mapped = map_usage(&usage, Some(&dedicated), Utc::now()).unwrap();
        assert_eq!(
            value_metric(&mapped, "rateLimitResets").values[0].number,
            3.0
        );
    }

    #[test]
    fn malformed_or_negative_reset_counts_are_omitted() {
        for body in [
            json!({"rate_limit_reset_credits": {"available_count": null}}),
            json!({"rate_limit_reset_credits": {"available_count": -1}}),
        ] {
            let mapped = map_usage(&response(body), None, Utc::now()).unwrap();
            assert!(mapped
                .value_metrics
                .iter()
                .all(|metric| metric.id != "rateLimitResets"));
        }
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
        let mapped = map_usage(&response, None, now).unwrap();
        assert_eq!(mapped.plan.as_deref(), Some("Plus"));
        assert_eq!(mapped.quotas.len(), 2);
        assert_eq!(mapped.quotas[0].used_percent, 37.5);
        assert_eq!(mapped.quotas[1].period_seconds, 604_800);
        assert!(mapped.quotas.iter().all(|quota| quota.resets_at.is_some()));
    }
}
