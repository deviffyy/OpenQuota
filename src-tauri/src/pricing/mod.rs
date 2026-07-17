mod catalog;
mod codecs;
mod model_pricing;
mod rates;
mod store;
mod supplement;

pub use catalog::PricingCatalog;
pub use model_pricing::ModelPricing;
pub use rates::{ModelRates, TokenBreakdown};
pub use store::PricingStore;
pub use supplement::PricingSupplement;

#[cfg(test)]
pub fn test_bundled_pricing() -> ModelPricing {
    ModelPricing::new(
        PricingSupplement::decode(include_bytes!("../../resources/pricing_supplement.json"))
            .expect("bundled pricing supplement must be valid"),
        codecs::catalog_from_compact(include_bytes!(
            "../../resources/pricing_litellm_snapshot.json"
        ))
        .expect("bundled LiteLLM pricing must be valid"),
        codecs::catalog_from_compact(include_bytes!(
            "../../resources/pricing_models_dev_snapshot.json"
        ))
        .expect("bundled models.dev pricing must be valid"),
    )
}

#[cfg(test)]
mod bundled_resource_tests {
    use super::{test_bundled_pricing, TokenBreakdown};

    #[test]
    fn bundled_catalogs_are_non_trivial_and_all_rules_resolve() {
        let pricing = test_bundled_pricing();
        assert!(pricing.primary.entries.len() > 500);
        assert!(pricing.secondary.entries.len() > 500);
        assert!(!pricing.supplement.pricing.is_empty());
        assert!(!pricing.supplement.alias_rules.is_empty());
        let raw_supplement: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../resources/pricing_supplement.json"))
                .unwrap();
        assert_eq!(
            pricing.supplement.alias_rules.len(),
            raw_supplement["alias_rules"].as_array().unwrap().len(),
            "every bundled alias regex must compile in Rust"
        );
        for rule in &pricing.supplement.alias_rules {
            assert!(
                pricing.resolve(&rule.canonical).is_some(),
                "alias canonical '{}' resolves against no pricing source",
                rule.canonical
            );
        }
        for base in pricing.supplement.fast_multipliers.keys() {
            assert!(
                pricing.resolve(base).is_some(),
                "fast multiplier base '{base}' resolves against no pricing source"
            );
        }
    }

    #[test]
    fn known_log_models_and_aliases_match_expected_rates() {
        let pricing = test_bundled_pricing();
        assert_eq!(
            pricing
                .resolve("claude-sonnet-4-5-20250929")
                .unwrap()
                .input_per_million,
            3.0
        );
        assert_eq!(
            pricing
                .resolve("gpt-5.6-sol-ultra-fast")
                .unwrap()
                .input_per_million,
            12.5
        );
        assert_eq!(
            pricing
                .resolve("claude-fable-5-thinking-xhigh")
                .unwrap()
                .output_per_million,
            50.0
        );
        assert_eq!(
            pricing.resolve("grok-build-0.1").unwrap().input_per_million,
            1.0
        );
        assert_eq!(
            pricing
                .resolve("cursor-grok-4.5-xhigh")
                .unwrap()
                .input_per_million,
            pricing.resolve("grok-4.5").unwrap().input_per_million
        );
        assert_eq!(
            pricing
                .resolve("cursor-grok-4.5-fast-xhigh")
                .unwrap()
                .output_per_million,
            pricing.resolve("grok-4.5-fast").unwrap().output_per_million
        );
        assert!(pricing.resolve("glm-5.2-bogus").is_none());
    }

    #[test]
    fn bundled_engine_prices_cache_and_long_context_buckets() {
        let pricing = test_bundled_pricing();
        let cost = pricing
            .estimated_cost_dollars(
                "claude-sonnet-4-5-20250929",
                TokenBreakdown {
                    input: 100_000,
                    cache_write_5m: 60_000,
                    cache_read: 50_000,
                    output: 20_000,
                    ..TokenBreakdown::default()
                },
                true,
            )
            .unwrap();
        assert!(cost > 0.0);
    }
}
