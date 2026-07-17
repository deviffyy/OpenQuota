use chrono::DateTime;
use serde_json::Value;

use crate::models::{QuotaFormat, QuotaWindow};

const MODEL_BLACKLIST: [&str; 9] = [
    "MODEL_CHAT_20706",
    "MODEL_CHAT_23310",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH_THINKING",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH_LITE",
    "MODEL_GOOGLE_GEMINI_2_5_PRO",
    "MODEL_PLACEHOLDER_M19",
    "MODEL_PLACEHOLDER_M9",
    "MODEL_PLACEHOLDER_M12",
];

#[derive(Debug)]
pub struct ModelConfig {
    label: String,
    model_id: Option<String>,
    remaining_fraction: f64,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

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

pub fn parse_user_status(value: &Value) -> Option<(Option<String>, Vec<ModelConfig>)> {
    value.get("userStatus")?;
    let configs = value
        .pointer("/userStatus/cascadeModelConfigData/clientModelConfigs")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(model_from_ls).collect())
        .unwrap_or_default();
    Some((parse_plan(value), configs))
}

pub fn parse_command_model_configs(value: &Value) -> Option<Vec<ModelConfig>> {
    Some(
        value
            .get("clientModelConfigs")?
            .as_array()?
            .iter()
            .filter_map(model_from_ls)
            .collect(),
    )
}

pub fn parse_cloud_models(value: &Value) -> Vec<ModelConfig> {
    let Some(models) = value.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|(key, model)| {
            if model.get("isInternal").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let label = model
                .get("displayName")
                .and_then(Value::as_str)
                .and_then(trimmed)
                .or_else(|| model.get("label").and_then(Value::as_str).and_then(trimmed))?;
            let model_id = model
                .get("model")
                .and_then(Value::as_str)
                .and_then(trimmed)
                .unwrap_or(key)
                .to_owned();
            Some(model_config(label, Some(model_id), model.get("quotaInfo")))
        })
        .collect()
}

pub fn parse_quota_buckets(value: &Value) -> Vec<ModelConfig> {
    value
        .get("buckets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|bucket| {
            let id = bucket
                .get("modelId")
                .and_then(Value::as_str)
                .and_then(trimmed)?
                .to_owned();
            Some(model_config(&id, Some(id.clone()), Some(bucket)))
        })
        .collect()
}

pub fn build_legacy_quotas(configs: Vec<ModelConfig>) -> Vec<QuotaWindow> {
    let mut gemini: Option<(f64, Option<chrono::DateTime<chrono::Utc>>)> = None;
    let mut claude: Option<(f64, Option<chrono::DateTime<chrono::Utc>>)> = None;
    for config in configs {
        if config
            .model_id
            .as_deref()
            .is_some_and(|id| MODEL_BLACKLIST.contains(&id))
        {
            continue;
        }
        let pool = if config.label.to_ascii_lowercase().contains("gemini") {
            &mut gemini
        } else {
            &mut claude
        };
        if pool
            .as_ref()
            .is_none_or(|(remaining, _)| config.remaining_fraction < *remaining)
        {
            *pool = Some((config.remaining_fraction, config.resets_at));
        }
    }
    [
        ("geminiPro", "Session", gemini),
        ("claude", "Claude", claude),
    ]
    .into_iter()
    .filter_map(|(id, label, pool)| {
        let (remaining, resets_at) = pool?;
        Some(QuotaWindow {
            id: id.into(),
            label: label.into(),
            used_percent: ((1.0 - remaining.clamp(0.0, 1.0)) * 100.0).round(),
            resets_at,
            period_seconds: 5 * 60 * 60,
            format: QuotaFormat::Percent,
            used_value: None,
            limit_value: None,
        })
    })
    .collect()
}

fn model_from_ls(value: &Value) -> Option<ModelConfig> {
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .and_then(trimmed)?;
    let model_id = value
        .pointer("/modelOrAlias/model")
        .and_then(Value::as_str)
        .and_then(trimmed)
        .map(str::to_owned);
    Some(model_config(label, model_id, value.get("quotaInfo")))
}

fn model_config(label: &str, model_id: Option<String>, quota: Option<&Value>) -> ModelConfig {
    ModelConfig {
        label: label.to_owned(),
        model_id,
        remaining_fraction: quota
            .and_then(|value| value.get("remainingFraction"))
            .and_then(number)
            .unwrap_or(0.0),
        resets_at: quota
            .and_then(|value| value.get("resetTime"))
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|date| date.to_utc()),
    }
}

fn trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
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
    use serde_json::{json, Value};

    use super::{
        build_legacy_quotas, parse_cloud_models, parse_command_model_configs, parse_quota_buckets,
        parse_quota_summary, parse_user_status,
    };

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

    #[test]
    fn pools_language_server_models_by_the_worst_remaining_fraction() {
        let value = json!({"userStatus": {
            "userTier": {"name": "Google AI Pro"},
            "cascadeModelConfigData": {"clientModelConfigs": [
                {"label": "Gemini 3 Pro", "quotaInfo": {"remainingFraction": 0.5}},
                {"label": "Gemini Flash", "quotaInfo": {"remainingFraction": 0.8}},
                {"label": "Claude Sonnet", "quotaInfo": {"remainingFraction": 0.7}}
            ]}
        }});
        let (plan, configs) = parse_user_status(&value).unwrap();
        let quotas = build_legacy_quotas(configs);
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert_eq!(
            quotas
                .iter()
                .map(|quota| quota.id.as_str())
                .collect::<Vec<_>>(),
            ["geminiPro", "claude"]
        );
        assert_eq!(quotas[0].used_percent, 50.0);
        assert_eq!(quotas[1].used_percent, 30.0);
    }

    #[test]
    fn command_models_cloud_models_and_quota_buckets_share_pooling_rules() {
        let command = json!({"clientModelConfigs": [
            {"label": "Claude Opus", "quotaInfo": {"remainingFraction": 0.25}}
        ]});
        assert_eq!(
            build_legacy_quotas(parse_command_model_configs(&command).unwrap())[0].id,
            "claude"
        );

        let cloud = json!({"models": {
            "good": {"model": "MODEL_GOOD", "displayName": "Gemini 3 Pro", "quotaInfo": {"remainingFraction": 0.4}},
            "internal": {"displayName": "Claude Internal", "isInternal": true},
            "blacklisted": {"model": "MODEL_GOOGLE_GEMINI_2_5_PRO", "displayName": "Gemini 2.5 Pro"}
        }});
        let quotas = build_legacy_quotas(parse_cloud_models(&cloud));
        assert_eq!(quotas.len(), 1);
        assert_eq!(quotas[0].used_percent, 60.0);

        let buckets = json!({"buckets": [
            {"modelId": "gemini-3-pro", "remainingFraction": 0.5},
            {"modelId": "gemini-3-flash", "remainingFraction": 0.9}
        ]});
        let quotas = build_legacy_quotas(parse_quota_buckets(&buckets));
        assert_eq!(quotas[0].id, "geminiPro");
        assert_eq!(quotas[0].used_percent, 50.0);
    }
}
