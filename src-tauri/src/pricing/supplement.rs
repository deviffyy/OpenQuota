use std::collections::HashMap;

use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

use super::{catalog::normalized_key, ModelRates};

#[derive(Debug, Default)]
pub struct PricingSupplement {
    pub pricing: HashMap<String, ModelRates>,
    pub fast_multipliers: HashMap<String, f64>,
    pub alias_rules: Vec<AliasRule>,
    pub updated_at: Option<String>,
}

#[derive(Debug)]
pub struct AliasRule {
    pub pattern: Regex,
    pub canonical: String,
}

impl PricingSupplement {
    pub fn decode(data: &[u8]) -> Result<Self, SupplementError> {
        let file = serde_json::from_slice::<SupplementFile>(data)?;
        let pricing = file
            .pricing
            .into_iter()
            .map(|(model, entry)| {
                let input = entry.input_per_million;
                let mut rates = ModelRates::new(input, entry.output_per_million);
                rates.cache_write_per_million = entry.cache_write_per_million.unwrap_or(input);
                rates.cache_read_per_million = entry.cache_read_per_million.unwrap_or(input * 0.1);
                (model, rates)
            })
            .collect();
        let alias_rules = file
            .alias_rules
            .into_iter()
            .filter_map(|rule| match Regex::new(&rule.pattern) {
                Ok(pattern) => Some(AliasRule {
                    pattern,
                    canonical: rule.canonical,
                }),
                Err(error) => {
                    crate::app_warn!(
                        "pricing",
                        "pricing supplement: invalid alias pattern '{}' skipped: {error}",
                        rule.pattern
                    );
                    None
                }
            })
            .collect();
        Ok(Self {
            pricing,
            fast_multipliers: file.fast_multipliers,
            alias_rules,
            updated_at: file.updated_at,
        })
    }

    pub fn canonical_name<'a>(&'a self, model: &str) -> Option<&'a str> {
        self.alias_rules
            .iter()
            .find(|rule| rule.pattern.is_match(model))
            .map(|rule| rule.canonical.as_str())
    }

    pub fn fast_multiplier(&self, model: &str) -> Option<f64> {
        if let Some(exact) = self.fast_multipliers.get(model) {
            return Some(*exact);
        }
        let normalized = normalized_key(model);
        for part in normalized.split(['/', ':']) {
            for (base, multiplier) in &self.fast_multipliers {
                let base = normalized_key(base);
                if matches_model_suffix(part, &base) {
                    return Some(*multiplier);
                }
            }
        }
        None
    }
}

fn matches_model_suffix(part: &str, base: &str) -> bool {
    part.rfind(base).is_some_and(|start| {
        let suffix = &part[start + base.len()..];
        suffix.is_empty() || suffix.starts_with('-')
    })
}

#[derive(Debug, Error)]
pub enum SupplementError {
    #[error("Pricing supplement is invalid JSON.")]
    InvalidJson(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct SupplementFile {
    #[serde(default)]
    updated_at: Option<String>,
    pricing: HashMap<String, SupplementEntry>,
    #[serde(default)]
    fast_multipliers: HashMap<String, f64>,
    #[serde(default)]
    alias_rules: Vec<SupplementRule>,
}

#[derive(Deserialize)]
struct SupplementEntry {
    input_per_million: f64,
    output_per_million: f64,
    #[serde(default)]
    cache_write_per_million: Option<f64>,
    #[serde(default)]
    cache_read_per_million: Option<f64>,
}

#[derive(Deserialize)]
struct SupplementRule {
    pattern: String,
    canonical: String,
}
