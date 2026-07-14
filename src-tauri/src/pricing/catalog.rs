use std::collections::HashMap;

use super::ModelRates;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PricingCatalog {
    pub entries: HashMap<String, ModelRates>,
    pub retrieved_at: Option<String>,
}

impl PricingCatalog {
    pub fn find_exact(&self, model: &str) -> Option<(&str, ModelRates)> {
        self.entries
            .get_key_value(model)
            .map(|(key, rates)| (key.as_str(), *rates))
    }

    pub fn find_fuzzy(&self, model: &str) -> Option<(&str, ModelRates)> {
        let normalized_model = normalized_key(model);
        self.entries
            .iter()
            .filter(|(key, _)| key_matches(key, model, &normalized_model))
            .map(|(key, rates)| (key.as_str(), *rates))
            .min_by(|(left, _), (right, _)| {
                right.len().cmp(&left.len()).then_with(|| left.cmp(right))
            })
    }

    pub fn merging(mut self, other: PricingCatalog) -> Self {
        self.entries.extend(other.entries);
        if other.retrieved_at.is_some() {
            self.retrieved_at = other.retrieved_at;
        }
        self
    }
}

pub fn normalized_key(value: &str) -> String {
    value.replace(['.', '@'], "-")
}

fn key_matches(candidate: &str, model: &str, normalized_model: &str) -> bool {
    if contains_key(model, candidate) || contains_key(candidate, model) {
        return true;
    }
    let normalized_candidate = normalized_key(candidate);
    contains_key(normalized_model, &normalized_candidate)
        || contains_key(&normalized_candidate, normalized_model)
}

fn contains_key(value: &str, key: &str) -> bool {
    if key.is_empty() || key.len() > value.len() {
        return false;
    }
    let value = value.as_bytes();
    let key = key.as_bytes();
    for start in 0..=value.len() - key.len() {
        if &value[start..start + key.len()] != key {
            continue;
        }
        if start > 0 && value[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        if suffix_allows_match(key, &value[start + key.len()..]) {
            return true;
        }
    }
    false
}

fn suffix_allows_match(key: &[u8], suffix: &[u8]) -> bool {
    let Some(separator) = suffix.first() else {
        return true;
    };
    if separator.is_ascii_alphanumeric() {
        return false;
    }
    !suffix_starts_with_numeric_model_version(key, suffix)
}

fn suffix_starts_with_numeric_model_version(key: &[u8], suffix: &[u8]) -> bool {
    if !key.last().is_some_and(u8::is_ascii_digit) || !matches!(suffix.first(), Some(b'-' | b'.')) {
        return false;
    }
    let digits = suffix[1..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return false;
    }
    let after_digits = suffix.get(1 + digits);
    let is_date = digits == 8 && after_digits.is_none_or(|byte| !byte.is_ascii_alphanumeric());
    !is_date
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::PricingCatalog;
    use crate::pricing::ModelRates;

    fn catalog(entries: &[(&str, f64)]) -> PricingCatalog {
        PricingCatalog {
            entries: entries
                .iter()
                .map(|(name, input)| ((*name).to_owned(), ModelRates::new(*input, 2.0)))
                .collect::<HashMap<_, _>>(),
            retrieved_at: None,
        }
    }

    #[test]
    fn fuzzy_matching_handles_dates_prefixes_and_separators() {
        let pricing = catalog(&[("claude-sonnet-4-20250514", 3.0), ("xai/grok-4.3", 1.25)]);
        assert_eq!(
            pricing
                .find_fuzzy("claude-sonnet-4")
                .unwrap()
                .1
                .input_per_million,
            3.0
        );
        assert_eq!(
            pricing.find_fuzzy("grok-4-3").unwrap().1.input_per_million,
            1.25
        );
    }

    #[test]
    fn numeric_versions_do_not_conflate() {
        let newer = catalog(&[("claude-sonnet-4-5", 3.0)]);
        assert!(newer.find_fuzzy("claude-sonnet-4").is_none());
        let older = catalog(&[("claude-sonnet-4", 1.0)]);
        assert!(older.find_fuzzy("claude-sonnet-4-5").is_none());
    }

    #[test]
    fn longest_key_wins_deterministically() {
        let pricing = catalog(&[("gemini-3-pro", 1.0), ("gemini/gemini-3-pro-preview", 2.0)]);
        assert_eq!(
            pricing
                .find_fuzzy("gemini-3-pro-preview")
                .unwrap()
                .1
                .input_per_million,
            2.0
        );
    }
}
