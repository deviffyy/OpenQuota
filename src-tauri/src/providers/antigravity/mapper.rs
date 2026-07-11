use chrono::DateTime;
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

const BUCKETS: [(&str, &str, &str, u64); 4] = [
    ("gemini-5h", "geminiPro", "Session", 5 * 60 * 60),
    ("gemini-weekly", "geminiWeekly", "Weekly", 7 * 24 * 60 * 60),
    ("3p-5h", "claude", "Claude", 5 * 60 * 60),
    (
        "3p-weekly",
        "claudeWeekly",
        "Claude Weekly",
        7 * 24 * 60 * 60,
    ),
];

pub fn parse_quota_summary(value: &Value) -> Option<Vec<QuotaWindow>> {
    let groups = value
        .pointer("/response/groups")
        .or_else(|| value.get("groups"))?
        .as_array()?;
    let mut found = std::collections::HashMap::new();
    for bucket in groups
        .iter()
        .filter_map(|group| group.get("buckets").and_then(Value::as_array))
        .flatten()
    {
        let Some(id) = bucket.get("bucketId").and_then(Value::as_str) else {
            continue;
        };
        if !BUCKETS.iter().any(|spec| spec.0 == id) || found.contains_key(id) {
            continue;
        }
        let Some(fraction) = bucket.get("remainingFraction").and_then(number) else {
            continue;
        };
        let reset = bucket
            .get("resetTime")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|date| date.to_utc());
        found.insert(id.to_owned(), (fraction, reset));
    }
    Some(
        BUCKETS
            .iter()
            .filter_map(|(bucket_id, quota_id, label, period_seconds)| {
                let (remaining, resets_at) = found.get(*bucket_id)?;
                Some(QuotaWindow {
                    id: (*quota_id).into(),
                    label: (*label).into(),
                    used_percent: ((1.0 - remaining.clamp(0.0, 1.0)) * 100.0).round(),
                    resets_at: *resets_at,
                    period_seconds: *period_seconds,
                    format: QuotaFormat::Percent,
                    used_value: None,
                    limit_value: None,
                })
            })
            .collect(),
    )
}

pub fn parse_plan(value: &Value) -> Option<String> {
    let raw = value
        .pointer("/userStatus/userTier/name")
        .or_else(|| value.pointer("/userStatus/planStatus/planInfo/planName"))
        .and_then(Value::as_str)?
        .trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(value) = raw.strip_prefix("Google AI ") {
        return Some(title_case(value));
    }
    for tier in ["Ultra", "Pro", "Free"] {
        if raw
            .to_ascii_lowercase()
            .contains(&tier.to_ascii_lowercase())
        {
            return Some(tier.into());
        }
    }
    Some(title_case(raw))
}

fn title_case(value: &str) -> String {
    value
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

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::parse_quota_summary;

    #[test]
    fn maps_authoritative_pool_summary_in_fixed_order() {
        let value: Value = serde_json::from_str(include_str!("fixtures/quota_summary.json"))
            .expect("valid fixture");
        let quotas = parse_quota_summary(&value).unwrap();
        assert_eq!(quotas.len(), 2);
        assert_eq!(quotas[0].id, "geminiPro");
        assert_eq!(quotas[0].used_percent, 20.0);
        assert_eq!(quotas[1].id, "claudeWeekly");
    }
}
