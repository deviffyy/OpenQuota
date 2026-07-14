use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::{ModelRates, PricingCatalog};

#[derive(Debug, Error)]
pub enum PricingCodecError {
    #[error("Pricing feed is not a JSON object.")]
    NotAnObject,
    #[error("Pricing feed contained no usable model entries.")]
    NoUsableEntries,
    #[error("Pricing feed is invalid JSON.")]
    InvalidJson(#[from] serde_json::Error),
}

pub fn catalog_from_litellm(data: &[u8]) -> Result<PricingCatalog, PricingCodecError> {
    let root = serde_json::from_slice::<Value>(data)?;
    let root = root.as_object().ok_or(PricingCodecError::NotAnObject)?;
    let mut entries = HashMap::new();
    for (key, value) in root {
        let Some(entry) = value.as_object() else {
            continue;
        };
        let (Some(input), Some(output)) = (
            number(entry.get("input_cost_per_token")),
            number(entry.get("output_cost_per_token")),
        ) else {
            continue;
        };
        let mut rates = ModelRates::new(input * 1_000_000.0, output * 1_000_000.0);
        rates.cache_write_per_million =
            number(entry.get("cache_creation_input_token_cost")).unwrap_or(input) * 1_000_000.0;
        rates.cache_read_per_million =
            number(entry.get("cache_read_input_token_cost")).unwrap_or(input * 0.1) * 1_000_000.0;
        rates.input_above_200k_per_million =
            number(entry.get("input_cost_per_token_above_200k_tokens"))
                .map(|rate| rate * 1_000_000.0);
        rates.output_above_200k_per_million =
            number(entry.get("output_cost_per_token_above_200k_tokens"))
                .map(|rate| rate * 1_000_000.0);
        rates.cache_write_above_200k_per_million =
            number(entry.get("cache_creation_input_token_cost_above_200k_tokens"))
                .map(|rate| rate * 1_000_000.0);
        rates.cache_read_above_200k_per_million =
            number(entry.get("cache_read_input_token_cost_above_200k_tokens"))
                .map(|rate| rate * 1_000_000.0);
        rates.fast_multiplier = entry
            .get("provider_specific_entry")
            .and_then(Value::as_object)
            .and_then(|specific| number(specific.get("fast")))
            .unwrap_or(1.0);
        entries.insert(key.clone(), rates);
    }
    if entries.is_empty() {
        return Err(PricingCodecError::NoUsableEntries);
    }
    Ok(PricingCatalog {
        entries,
        retrieved_at: None,
    })
}

pub fn catalog_from_models_dev(data: &[u8]) -> Result<PricingCatalog, PricingCodecError> {
    let root = serde_json::from_slice::<Value>(data)?;
    let root = root.as_object().ok_or(PricingCodecError::NotAnObject)?;
    let mut providers = root.keys().collect::<Vec<_>>();
    providers.sort();
    let mut entries = HashMap::new();
    for provider_name in providers {
        let Some(models) = root[provider_name].get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model_id, value) in models {
            if entries.contains_key(model_id) {
                continue;
            }
            let Some(cost) = value.get("cost").and_then(Value::as_object) else {
                continue;
            };
            let (Some(input), Some(output)) =
                (number(cost.get("input")), number(cost.get("output")))
            else {
                continue;
            };
            let mut rates = ModelRates::new(input, output);
            rates.cache_write_per_million = number(cost.get("cache_write")).unwrap_or(input);
            rates.cache_read_per_million = number(cost.get("cache_read")).unwrap_or(input * 0.1);
            entries.insert(model_id.clone(), rates);
        }
    }
    if entries.is_empty() {
        return Err(PricingCodecError::NoUsableEntries);
    }
    Ok(PricingCatalog {
        entries,
        retrieved_at: None,
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    value?.as_f64()
}

#[derive(Debug, Serialize, Deserialize)]
struct CompactCatalog {
    #[serde(default)]
    retrieved_at: Option<String>,
    models: BTreeMap<String, CompactModel>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompactModel {
    i: f64,
    o: f64,
    cw: f64,
    cr: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ia: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oa: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwa: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cra: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fast: Option<f64>,
}

pub fn catalog_from_compact(data: &[u8]) -> Result<PricingCatalog, PricingCodecError> {
    let file = serde_json::from_slice::<CompactCatalog>(data)?;
    let entries = file
        .models
        .into_iter()
        .map(|(key, model)| {
            (
                key,
                ModelRates {
                    input_per_million: model.i,
                    output_per_million: model.o,
                    cache_write_per_million: model.cw,
                    cache_read_per_million: model.cr,
                    input_above_200k_per_million: model.ia,
                    output_above_200k_per_million: model.oa,
                    cache_write_above_200k_per_million: model.cwa,
                    cache_read_above_200k_per_million: model.cra,
                    fast_multiplier: model.fast.unwrap_or(1.0),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    Ok(PricingCatalog {
        entries,
        retrieved_at: file.retrieved_at,
    })
}

pub fn compact_data(catalog: &PricingCatalog) -> Result<Vec<u8>, PricingCodecError> {
    let models = catalog
        .entries
        .iter()
        .map(|(key, rates)| {
            (
                key.clone(),
                CompactModel {
                    i: rates.input_per_million,
                    o: rates.output_per_million,
                    cw: rates.cache_write_per_million,
                    cr: rates.cache_read_per_million,
                    ia: rates.input_above_200k_per_million,
                    oa: rates.output_above_200k_per_million,
                    cwa: rates.cache_write_above_200k_per_million,
                    cra: rates.cache_read_above_200k_per_million,
                    fast: (rates.fast_multiplier != 1.0).then_some(rates.fast_multiplier),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(serde_json::to_vec(&CompactCatalog {
        retrieved_at: catalog.retrieved_at.clone(),
        models,
    })?)
}

#[cfg(test)]
mod tests {
    use super::{catalog_from_litellm, catalog_from_models_dev, compact_data};

    #[test]
    fn parses_litellm_defaults_and_round_trips_compact_data() {
        let feed = br#"{"model":{"input_cost_per_token":0.000003,"output_cost_per_token":0.000015,"provider_specific_entry":{"fast":6}}}"#;
        let catalog = catalog_from_litellm(feed).unwrap();
        let rates = catalog.entries["model"];
        assert_eq!(rates.input_per_million, 3.0);
        assert_eq!(rates.cache_write_per_million, 3.0);
        assert!((rates.cache_read_per_million - 0.3).abs() < 0.000_001);
        assert_eq!(rates.fast_multiplier, 6.0);
        let compact = compact_data(&catalog).unwrap();
        assert_eq!(super::catalog_from_compact(&compact).unwrap(), catalog);
    }

    #[test]
    fn models_dev_uses_first_provider_in_name_order() {
        let feed = br#"{"z":{"models":{"shared":{"cost":{"input":9,"output":9}}}},"a":{"models":{"shared":{"cost":{"input":1,"output":2}}}}}"#;
        let catalog = catalog_from_models_dev(feed).unwrap();
        assert_eq!(catalog.entries["shared"].input_per_million, 1.0);
    }
}
