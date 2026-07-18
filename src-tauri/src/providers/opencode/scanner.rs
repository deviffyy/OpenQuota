use std::{collections::HashMap, path::PathBuf};

use chrono::{DateTime, Days, Local, Utc};

use crate::{
    models::UsageHistory, pricing::ModelPricing, providers::daily_usage::DailyUsageAccumulator,
};

use super::{
    database::{has_hosted_usage, read_database, DatabaseRead},
    paths::OpenCodePaths,
    record::{CostProvenance, UsageRecord, GO_PROVIDER_ID},
    windows::OpenCodeWindows,
    OpenCodeError,
};

const SCAN_DAYS: i64 = 33;
pub(crate) const USAGE_SOURCE_NOTE: &str =
    "From your OpenCode local database; missing costs use catalog estimates";

#[derive(Debug)]
pub(crate) struct OpenCodeUsageScan {
    pub(crate) usage: UsageHistory,
    pub(crate) go_windows: Option<OpenCodeWindows>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenCodeUsageScanner {
    paths: OpenCodePaths,
}

impl OpenCodeUsageScanner {
    pub(crate) fn new(paths: OpenCodePaths) -> Self {
        Self { paths }
    }

    #[cfg(test)]
    pub(crate) fn for_paths(paths: Vec<PathBuf>) -> Self {
        let data_directory = paths
            .first()
            .and_then(|path| path.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        Self::new(OpenCodePaths::for_data_directory(data_directory))
    }

    pub(crate) fn scan(
        &self,
        now: DateTime<Utc>,
        has_go_key: bool,
        pricing: &ModelPricing,
    ) -> Result<Option<OpenCodeUsageScan>, OpenCodeError> {
        let paths = self.paths.database_files()?;
        self.scan_paths(paths, now, has_go_key, pricing)
    }

    pub(crate) fn scan_paths(
        &self,
        mut paths: Vec<PathBuf>,
        now: DateTime<Utc>,
        has_go_key: bool,
        pricing: &ModelPricing,
    ) -> Result<Option<OpenCodeUsageScan>, OpenCodeError> {
        paths.sort();
        paths.dedup();
        self.scan_sorted_paths(paths, now, has_go_key, pricing)
    }

    fn scan_sorted_paths(
        &self,
        paths: Vec<PathBuf>,
        now: DateTime<Utc>,
        has_go_key: bool,
        pricing: &ModelPricing,
    ) -> Result<Option<OpenCodeUsageScan>, OpenCodeError> {
        if paths.is_empty() {
            return Ok(None);
        }

        let cutoff_ms = (now - chrono::Duration::days(SCAN_DAYS)).timestamp_millis();
        let mut records = Vec::new();
        let mut go_anchor = None;
        let mut usable_databases = 0_usize;
        let mut failed_databases = 0_usize;
        for path in paths {
            match read_database(&path, cutoff_ms, pricing) {
                Ok(DatabaseRead::Missing) => {}
                Ok(DatabaseRead::Usable(database)) => {
                    usable_databases += 1;
                    records.extend(database.records);
                    if let Some(candidate) = database.go_anchor {
                        go_anchor =
                            Some(go_anchor.map_or(candidate, |current: DateTime<Utc>| {
                                current.min(candidate)
                            }));
                    }
                }
                Err(()) => failed_databases += 1,
            }
        }
        if usable_databases == 0 {
            return if failed_databases == 0 {
                Ok(None)
            } else {
                Err(OpenCodeError::DatabaseUnreadable)
            };
        }

        let records = deduplicate(records);
        let current_records = records
            .iter()
            .filter(|record| record.timestamp <= now)
            .collect::<Vec<_>>();
        let go_activity = current_records.iter().any(|record| {
            record.provider_id == GO_PROVIDER_ID
                && record.cost_provenance == CostProvenance::Exact
                && record.cost.is_some()
        });
        let go_costs = current_records
            .iter()
            .filter(|record| {
                record.provider_id == GO_PROVIDER_ID
                    && record.cost_provenance == CostProvenance::Exact
            })
            .filter_map(|record| record.cost.map(|cost| (record.timestamp, cost)))
            .collect::<Vec<_>>();
        let go_windows = (has_go_key || go_activity)
            .then(|| OpenCodeWindows::compute(&go_costs, go_anchor, now));

        let mut warnings = Vec::new();
        if failed_databases > 0 {
            crate::app_warn!(
                "plugin:opencode",
                "{failed_databases} local database(s) could not be read; usable sources remain"
            );
            warnings.push(
                "Some OpenCode databases could not be read; available local usage is shown.".into(),
            );
        }

        Ok(Some(OpenCodeUsageScan {
            usage: aggregate_history(&records, now),
            go_windows,
            warnings,
        }))
    }

    pub(crate) fn has_hosted_usage(&self) -> bool {
        let paths = match self.paths.database_files() {
            Ok(paths) => paths,
            Err(_) => {
                crate::app_warn!(
                    "plugin:opencode",
                    "usage probe could not enumerate the local data directory"
                );
                return true;
            }
        };
        paths.iter().any(|path| match has_hosted_usage(path) {
            Ok(found) => found,
            Err(()) => {
                crate::app_warn!(
                    "plugin:opencode",
                    "usage probe could not read one local database"
                );
                false
            }
        })
    }
}

fn deduplicate(records: Vec<UsageRecord>) -> Vec<UsageRecord> {
    let mut deduplicated = HashMap::<(String, String), UsageRecord>::new();
    for candidate in records {
        match deduplicated.entry(candidate.key.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if better_record(&candidate, entry.get()) {
                    entry.insert(candidate);
                }
            }
        }
    }
    let mut records = deduplicated.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.key.cmp(&right.key))
    });
    records
}

fn better_record(candidate: &UsageRecord, current: &UsageRecord) -> bool {
    let quality = |record: &UsageRecord| {
        (
            record.cost.is_some() && !record.incomplete_cost,
            record.cost.is_some(),
            record.tokens,
            record.cost_provenance == CostProvenance::Exact,
        )
    };
    quality(candidate) > quality(current)
        || (quality(candidate) == quality(current)
            && candidate.cost.unwrap_or_default() > current.cost.unwrap_or_default())
}

fn aggregate_history(records: &[UsageRecord], now: DateTime<Utc>) -> UsageHistory {
    let today = now.with_timezone(&Local).date_naive();
    let since = today.checked_sub_days(Days::new(30)).unwrap_or(today);
    let mut accumulator = DailyUsageAccumulator::default();
    for record in records {
        if record.timestamp > now {
            continue;
        }
        let date = record.timestamp.with_timezone(&Local).date_naive();
        if date < since {
            continue;
        }
        if let Some(cost) = record.cost {
            match record.cost_provenance {
                CostProvenance::Exact => {
                    accumulator.add_exact(date, record.tokens, cost, &record.model)
                }
                CostProvenance::Estimated => {
                    accumulator.add(date, record.tokens, cost, &record.model)
                }
            }
        }
        if record.incomplete_cost || (record.cost.is_none() && record.tokens > 0) {
            accumulator.add_unknown_model(date, &record.model);
        }
    }
    accumulator.build(now, USAGE_SOURCE_NOTE)
}

#[cfg(test)]
mod unit_tests {
    use chrono::{TimeZone, Utc};

    use super::{aggregate_history, deduplicate};
    use crate::providers::opencode::record::{CostProvenance, UsageRecord};

    fn record(tokens: u64, cost: Option<f64>, exact: bool) -> UsageRecord {
        UsageRecord {
            key: ("session".into(), "message".into()),
            timestamp: Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0).unwrap(),
            provider_id: "opencode".into(),
            model: "model".into(),
            tokens,
            cost,
            cost_provenance: if exact {
                CostProvenance::Exact
            } else {
                CostProvenance::Estimated
            },
            incomplete_cost: cost.is_none(),
        }
    }

    #[test]
    fn duplicate_messages_choose_the_most_complete_deterministically() {
        let records = deduplicate(vec![
            record(100, Some(1.0), false),
            record(200, Some(2.0), true),
            record(300, None, false),
        ]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tokens, 200);
        assert_eq!(records[0].cost, Some(2.0));
        assert_eq!(records[0].cost_provenance, CostProvenance::Exact);
    }

    #[test]
    fn exact_and_estimated_costs_keep_their_period_provenance() {
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();
        let exact = aggregate_history(&[record(100, Some(1.0), true)], now);
        assert!(!exact.today.unwrap().cost_estimated);

        let estimated = aggregate_history(&[record(100, Some(1.0), false)], now);
        assert!(estimated.today.unwrap().cost_estimated);
    }
}
