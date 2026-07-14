#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelRates {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_write_per_million: f64,
    pub cache_read_per_million: f64,
    pub input_above_200k_per_million: Option<f64>,
    pub output_above_200k_per_million: Option<f64>,
    pub cache_write_above_200k_per_million: Option<f64>,
    pub cache_read_above_200k_per_million: Option<f64>,
    pub fast_multiplier: f64,
}

impl ModelRates {
    pub fn new(input_per_million: f64, output_per_million: f64) -> Self {
        Self {
            input_per_million,
            output_per_million,
            cache_write_per_million: input_per_million,
            cache_read_per_million: input_per_million * 0.1,
            input_above_200k_per_million: None,
            output_above_200k_per_million: None,
            cache_write_above_200k_per_million: None,
            cache_read_above_200k_per_million: None,
            fast_multiplier: 1.0,
        }
    }

    pub fn scaled(self, factor: f64) -> Self {
        Self {
            input_per_million: self.input_per_million * factor,
            output_per_million: self.output_per_million * factor,
            cache_write_per_million: self.cache_write_per_million * factor,
            cache_read_per_million: self.cache_read_per_million * factor,
            input_above_200k_per_million: self
                .input_above_200k_per_million
                .map(|rate| rate * factor),
            output_above_200k_per_million: self
                .output_above_200k_per_million
                .map(|rate| rate * factor),
            cache_write_above_200k_per_million: self
                .cache_write_above_200k_per_million
                .map(|rate| rate * factor),
            cache_read_above_200k_per_million: self
                .cache_read_above_200k_per_million
                .map(|rate| rate * factor),
            fast_multiplier: 1.0,
        }
    }

    pub fn cost_dollars(self, tokens: TokenBreakdown, apply_long_context_rates: bool) -> f64 {
        let use_long_context = apply_long_context_rates && tokens.prompt_tokens() > 200_000;
        let select = |base: f64, long_context: Option<f64>| {
            if use_long_context {
                long_context.unwrap_or(base)
            } else {
                base
            }
        };
        let input_rate = select(self.input_per_million, self.input_above_200k_per_million);
        let output_rate = select(self.output_per_million, self.output_above_200k_per_million);
        let cache_write_rate = select(
            self.cache_write_per_million,
            self.cache_write_above_200k_per_million,
        );
        let cache_read_rate = select(
            self.cache_read_per_million,
            self.cache_read_above_200k_per_million,
        );
        let cache_write_1h_rate = input_rate * 2.0;
        let cost = tokens.input as f64 * input_rate
            + tokens.output as f64 * output_rate
            + tokens.cache_write_5m as f64 * cache_write_rate
            + tokens.cache_write_1h as f64 * cache_write_1h_rate
            + tokens.cache_read as f64 * cache_read_rate;
        cost / 1_000_000.0
            * if tokens.is_fast {
                self.fast_multiplier
            } else {
                1.0
            }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenBreakdown {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    pub is_fast: bool,
}

impl TokenBreakdown {
    pub fn prompt_tokens(self) -> u64 {
        self.input
            .saturating_add(self.cache_write_5m)
            .saturating_add(self.cache_write_1h)
            .saturating_add(self.cache_read)
    }

    pub fn total_tokens(self) -> u64 {
        self.prompt_tokens().saturating_add(self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelRates, TokenBreakdown};

    #[test]
    fn cost_uses_all_buckets_and_one_hour_cache_rate() {
        let mut rates = ModelRates::new(3.0, 15.0);
        rates.cache_write_per_million = 3.75;
        rates.cache_read_per_million = 0.3;
        let tokens = TokenBreakdown {
            input: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            cache_read: 1_000_000,
            output: 1_000_000,
            is_fast: false,
        };
        assert!((rates.cost_dollars(tokens, true) - 28.05).abs() < 0.000_001);
    }

    #[test]
    fn combined_prompt_selects_request_wide_long_context_rates() {
        let mut rates = ModelRates::new(3.0, 15.0);
        rates.cache_write_per_million = 3.75;
        rates.cache_read_per_million = 0.3;
        rates.input_above_200k_per_million = Some(6.0);
        rates.output_above_200k_per_million = Some(22.5);
        rates.cache_write_above_200k_per_million = Some(7.5);
        rates.cache_read_above_200k_per_million = Some(0.6);
        let tokens = TokenBreakdown {
            input: 100_000,
            cache_write_5m: 60_000,
            cache_read: 50_000,
            output: 20_000,
            ..TokenBreakdown::default()
        };
        assert!((rates.cost_dollars(tokens, true) - 1.53).abs() < 0.000_001);
        assert!(rates.cost_dollars(tokens, false) < 1.53);
    }

    #[test]
    fn exactly_200k_keeps_base_rates_and_fast_scales_cost() {
        let mut rates = ModelRates::new(3.0, 15.0);
        rates.input_above_200k_per_million = Some(6.0);
        rates.fast_multiplier = 2.5;
        let tokens = TokenBreakdown {
            input: 200_000,
            output: 10_000,
            is_fast: true,
            ..TokenBreakdown::default()
        };
        assert!((rates.cost_dollars(tokens, true) - 1.875).abs() < 0.000_001);
    }

    #[test]
    fn large_output_alone_does_not_select_long_context_rates() {
        let mut rates = ModelRates::new(3.0, 15.0);
        rates.input_above_200k_per_million = Some(6.0);
        rates.output_above_200k_per_million = Some(22.5);
        let tokens = TokenBreakdown {
            input: 10_000,
            output: 300_000,
            ..TokenBreakdown::default()
        };
        assert!((rates.cost_dollars(tokens, true) - 4.53).abs() < 0.000_001);
    }
}
