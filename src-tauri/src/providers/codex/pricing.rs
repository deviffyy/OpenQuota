use super::local_usage::TokenEvent;

#[derive(Debug, Clone, Copy)]
pub struct ModelRates {
    input: f64,
    cached_input: Option<f64>,
    output: f64,
    long_context: bool,
    priority_multiplier: Option<f64>,
}

pub fn estimate_cost(event: &TokenEvent, fast_tier: bool) -> Option<f64> {
    let rates = rates_for(&event.model)?;
    let non_cached = event.input.saturating_sub(event.cached) as f64;
    let cached = event.cached as f64;
    let output = event.output as f64;
    let uses_long_context_price = rates.long_context && event.input > 272_000;
    let (input_multiplier, output_multiplier) = if uses_long_context_price {
        (2.0, 1.5)
    } else {
        (1.0, 1.0)
    };
    let cached_input = if event.cached == 0 {
        0.0
    } else {
        cached * rates.cached_input?
    };
    let service_multiplier = if fast_tier {
        // OpenAI currently publishes Priority pricing for short context only.
        if uses_long_context_price {
            return None;
        }
        rates.priority_multiplier?
    } else {
        1.0
    };
    Some(
        (non_cached * rates.input * input_multiplier
            + cached_input * input_multiplier
            + output * rates.output * output_multiplier)
            / 1_000_000.0
            * service_multiplier,
    )
}

fn rates_for(model: &str) -> Option<ModelRates> {
    let normalized = model.trim().to_ascii_lowercase();
    // Standard, direct-OpenAI prices per 1M tokens. Dated snapshots inherit
    // the price of their public model family.
    let values = if normalized == "codex-mini-latest" {
        (1.50, Some(0.375), 6.0, false, Some(2.0))
    } else if model_family(&normalized, "gpt-5.6-sol") {
        (5.0, Some(0.50), 30.0, true, Some(2.0))
    } else if model_family(&normalized, "gpt-5.6-terra") {
        (2.50, Some(0.25), 15.0, true, Some(2.0))
    } else if model_family(&normalized, "gpt-5.6-luna") {
        (1.0, Some(0.10), 6.0, true, Some(2.0))
    } else if model_family(&normalized, "gpt-5.5-pro") {
        (30.0, None, 180.0, true, None)
    } else if model_family(&normalized, "gpt-5.5") {
        (5.0, Some(0.50), 30.0, true, Some(2.5))
    } else if model_family(&normalized, "gpt-5.4-mini") {
        (0.75, Some(0.075), 4.50, false, Some(2.0))
    } else if model_family(&normalized, "gpt-5.4-nano") {
        (0.20, Some(0.02), 1.25, false, None)
    } else if model_family(&normalized, "gpt-5.4-pro") {
        (30.0, None, 180.0, true, None)
    } else if model_family(&normalized, "gpt-5.4") {
        (2.50, Some(0.25), 15.0, true, Some(2.0))
    } else if normalized.starts_with("gpt-5.3") || normalized.starts_with("gpt-5.2") {
        (1.75, Some(0.175), 14.0, false, Some(2.0))
    } else if normalized.starts_with("gpt-5.1-codex-mini") {
        (0.25, Some(0.025), 2.0, false, Some(2.0))
    } else if normalized == "gpt-5"
        || normalized.starts_with("gpt-5-")
        || normalized.starts_with("gpt-5.1")
    {
        (1.25, Some(0.125), 10.0, false, Some(2.0))
    } else {
        return None;
    };
    Some(ModelRates {
        input: values.0,
        cached_input: values.1,
        output: values.2,
        long_context: values.3,
        priority_multiplier: values.4,
    })
}

fn model_family(model: &str, family: &str) -> bool {
    model == family
        || model
            .strip_prefix(family)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .is_some_and(|snapshot| {
                let bytes = snapshot.as_bytes();
                bytes.len() >= 10
                    && bytes[..4].iter().all(u8::is_ascii_digit)
                    && bytes[4] == b'-'
                    && bytes[5..7].iter().all(u8::is_ascii_digit)
                    && bytes[7] == b'-'
                    && bytes[8..10].iter().all(u8::is_ascii_digit)
            })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::estimate_cost;
    use crate::providers::codex::local_usage::TokenEvent;

    #[test]
    fn cached_tokens_use_discounted_rate() {
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.3-codex".into(),
            input: 1_000,
            cached: 400,
            output: 100,
            reasoning: 0,
            total: 1_100,
        };
        let expected = (600.0 * 1.75 + 400.0 * 0.175 + 100.0 * 14.0) / 1_000_000.0;
        assert!((estimate_cost(&event, false).unwrap() - expected).abs() < 0.000_001);
    }

    #[test]
    fn gpt_5_6_sol_uses_official_standard_rates() {
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.6-sol".into(),
            input: 100_000,
            cached: 10_000,
            output: 10_000,
            reasoning: 0,
            total: 2,
        };
        let expected = 90_000.0 * 5.0 / 1_000_000.0
            + 10_000.0 * 0.5 / 1_000_000.0
            + 10_000.0 * 30.0 / 1_000_000.0;
        assert!((estimate_cost(&event, false).unwrap() - expected).abs() < 0.000_001);
    }

    #[test]
    fn new_model_variants_do_not_fall_through_to_the_base_price() {
        let event = |model: &str| TokenEvent {
            timestamp: Utc::now(),
            model: model.into(),
            input: 100_000,
            cached: 0,
            output: 0,
            reasoning: 0,
            total: 100_000,
        };

        assert_eq!(estimate_cost(&event("gpt-5.6-terra"), false), Some(0.25));
        assert_eq!(estimate_cost(&event("gpt-5.6-luna"), false), Some(0.1));
        assert_eq!(estimate_cost(&event("gpt-5.4-mini"), false), Some(0.075));
        assert_eq!(estimate_cost(&event("gpt-5.4-nano"), false), Some(0.02));
        assert_eq!(estimate_cost(&event("gpt-5.4-cyber"), false), None);
    }

    #[test]
    fn priority_rate_follows_the_official_short_context_table() {
        let event = TokenEvent {
            timestamp: Utc::now(),
            model: "gpt-5.5".into(),
            input: 200_000,
            cached: 0,
            output: 100_000,
            reasoning: 0,
            total: 300_000,
        };
        let expected = (200_000.0 * 5.0 + 100_000.0 * 30.0) / 1_000_000.0 * 2.5;
        assert!((estimate_cost(&event, true).unwrap() - expected).abs() < 0.000_001);
    }

    #[test]
    fn unpublished_priority_prices_stay_unpriced() {
        let event = |model: &str, input| TokenEvent {
            timestamp: Utc::now(),
            model: model.into(),
            input,
            cached: 0,
            output: 1,
            reasoning: 0,
            total: input + 1,
        };

        assert_eq!(estimate_cost(&event("gpt-5.6-sol", 300_000), true), None);
        assert_eq!(estimate_cost(&event("gpt-5.5-pro", 100), true), None);
        assert_eq!(estimate_cost(&event("gpt-5.4-nano", 100), true), None);
    }
}
