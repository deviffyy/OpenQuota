use std::collections::HashMap;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

use crate::pricing::{ModelPricing, TokenBreakdown};

#[derive(Debug, Clone, PartialEq)]
pub struct CursorCsvRow {
    pub date: DateTime<Utc>,
    pub model: String,
    pub tokens: TokenBreakdown,
    pub estimated_cost_usd: Option<f64>,
}

pub fn parse_usage_csv(csv: &str, pricing: &ModelPricing) -> Vec<CursorCsvRow> {
    records(csv)
        .into_iter()
        .filter_map(|record| {
            let date = parse_date(record.get("Date")?.trim())?;
            let model = record
                .get("Model")
                .map_or("", String::as_str)
                .trim()
                .to_owned();
            let tokens = TokenBreakdown {
                input: parse_integer(record.get("Input (w/o Cache Write)")),
                cache_write_5m: parse_integer(record.get("Input (w/ Cache Write)")),
                cache_read: parse_integer(record.get("Cache Read")),
                output: parse_integer(record.get("Output Tokens")),
                ..TokenBreakdown::default()
            };
            let estimated_cost_usd = pricing.estimated_cost_dollars(&model, tokens, false);
            Some(CursorCsvRow {
                date,
                model,
                tokens,
                estimated_cost_usd,
            })
        })
        .collect()
}

fn records(csv: &str) -> Vec<HashMap<String, String>> {
    let rows = parse_rows(csv);
    let Some(headers) = rows.first() else {
        return Vec::new();
    };
    rows.iter()
        .skip(1)
        .filter(|row| row.iter().any(|field| !field.is_empty()))
        .map(|row| {
            headers
                .iter()
                .enumerate()
                .filter(|(_, header)| !header.is_empty())
                .map(|(index, header)| {
                    (header.clone(), row.get(index).cloned().unwrap_or_default())
                })
                .collect()
        })
        .collect()
}

fn parse_rows(csv: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = csv.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if in_quotes => in_quotes = false,
            '"' if field.is_empty() => in_quotes = true,
            ',' if !in_quotes => {
                row.push(std::mem::take(&mut field));
            }
            '\n' if !in_quotes => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !in_quotes && chars.peek() == Some(&'\n') => {}
            other => field.push(other),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|date| date.to_utc())
        .or_else(|| {
            let naive = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S").ok()?;
            Local
                .from_local_datetime(&naive)
                .single()
                .or_else(|| Local.from_local_datetime(&naive).earliest())
                .map(|date| date.to_utc())
        })
}

fn parse_integer(value: Option<&String>) -> u64 {
    value
        .map(|value| value.replace(',', ""))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::pricing::{ModelRates, PricingCatalog, PricingSupplement};

    use super::*;

    fn pricing() -> ModelPricing {
        ModelPricing::new(
            PricingSupplement::default(),
            PricingCatalog {
                entries: HashMap::from([("composer-1".into(), ModelRates::new(2.0, 10.0))]),
                retrieved_at: None,
            },
            PricingCatalog::default(),
        )
    }

    #[test]
    fn parser_handles_quoted_commas_quotes_newlines_and_partial_last_row() {
        let csv = "Date,Model,Note\n2026-01-01T00:00:00Z,composer-1,\"a, b \"\"quoted\"\" c\"\n2026-01-02T00:00:00Z,composer-1,\"line one\nline two\"";
        let records = records(csv);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["Note"], "a, b \"quoted\" c");
        assert_eq!(records[1]["Note"], "line one\nline two");
    }

    #[test]
    fn cursor_columns_are_priced_without_long_context_uplift() {
        let csv = "Date,Model,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens\n2026-01-01T00:00:00Z,composer-1,0,300,000,0,1000000";
        // Quoted thousands are the valid CSV representation of a comma-bearing number.
        let csv = csv.replace(",300,000,", ",\"300,000\",");
        let rows = parse_usage_csv(&csv, &pricing());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tokens.input, 300_000);
        assert!((rows[0].estimated_cost_usd.unwrap() - 10.6).abs() < 0.000_001);
    }

    #[test]
    fn unknown_pricing_keeps_tokens_and_marks_cost_missing() {
        let csv =
            "Date,Model,Input (w/o Cache Write),Output Tokens\n2026-01-01 00:00:00,unknown,100,0";
        let row = parse_usage_csv(csv, &pricing()).pop().unwrap();
        assert_eq!(row.tokens.total_tokens(), 100);
        assert!(row.estimated_cost_usd.is_none());
    }
}
