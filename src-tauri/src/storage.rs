use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use crate::{
    models::{AppSettings, DailyUsage, ProviderSnapshot},
    providers::CacheIdentity,
};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("OpenQuota data directory could not be created")]
    CreateDirectory(#[source] std::io::Error),
    #[error("OpenQuota database is unavailable")]
    Database(#[from] rusqlite::Error),
    #[error("Cached OpenQuota data is invalid")]
    InvalidCache(#[from] serde_json::Error),
    #[error("OpenQuota database lock is unavailable")]
    Poisoned,
}

pub struct Storage {
    connection: Mutex<Connection>,
}

pub struct ProviderAccountUpdate {
    pub provider_family: String,
    pub identity_key: String,
    pub provider_id: String,
    pub payload: String,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(StorageError::CreateDirectory)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS provider_snapshots (
               provider_id TEXT PRIMARY KEY,
               payload TEXT NOT NULL,
               refreshed_at TEXT NOT NULL,
               identity_key TEXT
             );
             CREATE TABLE IF NOT EXISTS daily_usage (
               provider_id TEXT NOT NULL,
               date TEXT NOT NULL,
               tokens INTEGER NOT NULL,
               estimated_cost_usd REAL,
               estimate_complete INTEGER NOT NULL,
               PRIMARY KEY(provider_id, date)
             );
             CREATE TABLE IF NOT EXISTS log_file_cache (
               provider_id TEXT NOT NULL,
               path TEXT NOT NULL,
               size INTEGER NOT NULL,
               modified_nanos INTEGER NOT NULL,
               events_json TEXT NOT NULL,
               PRIMARY KEY(provider_id, path)
             );
             CREATE TABLE IF NOT EXISTS app_settings (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               payload TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS panel_state (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               height INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS provider_environment (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               payload TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS provider_account_records (
               provider_family TEXT NOT NULL,
               identity_key TEXT NOT NULL,
               provider_id TEXT NOT NULL,
               payload TEXT NOT NULL,
               PRIMARY KEY(provider_family, identity_key),
               UNIQUE(provider_family, provider_id)
             );",
        )?;
        if !Self::has_column(&connection, "log_file_cache", "modified_nanos")? {
            // Parsed log rows are disposable. Rebuilding the table is safer than converting the old
            // millisecond timestamp because the conversion would preserve the very collisions this
            // migration removes. The next refresh repopulates it from the source logs.
            connection.execute_batch(
                "DROP TABLE log_file_cache;
                 CREATE TABLE log_file_cache (
                   provider_id TEXT NOT NULL,
                   path TEXT NOT NULL,
                   size INTEGER NOT NULL,
                   modified_nanos INTEGER NOT NULL,
                   events_json TEXT NOT NULL,
                   PRIMARY KEY(provider_id, path)
                 );",
            )?;
        }
        if !Self::has_column(&connection, "provider_snapshots", "identity_key")? {
            connection.execute(
                "ALTER TABLE provider_snapshots ADD COLUMN identity_key TEXT",
                [],
            )?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    #[cfg(test)]
    pub fn load_snapshot(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderSnapshot>, StorageError> {
        self.load_snapshot_for_identity(provider_id, CacheIdentity::Unscoped)
    }

    pub fn load_snapshot_for_identity(
        &self,
        provider_id: &str,
        identity: CacheIdentity<'_>,
    ) -> Result<Option<ProviderSnapshot>, StorageError> {
        let connection = self.connection()?;
        let cached: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT payload, identity_key FROM provider_snapshots WHERE provider_id = ?1",
                [provider_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        cached
            .filter(|(_, cached_identity)| match identity {
                CacheIdentity::Unscoped => cached_identity.is_none(),
                CacheIdentity::Resolved(expected) => cached_identity.as_deref() == Some(expected),
                CacheIdentity::Unresolved => true,
            })
            .map(|json| {
                let mut snapshot: ProviderSnapshot = serde_json::from_str(&json.0)?;
                // Count quotas predate the unit field. The only count quota persisted by older
                // releases was request-based, so normalize it once at the cache boundary instead of
                // teaching every presentation surface to infer provider semantics.
                for quota in &mut snapshot.quotas {
                    if quota.format == crate::models::QuotaFormat::Count && quota.unit.is_none() {
                        quota.unit = Some("requests".into());
                    }
                }
                Ok(snapshot)
            })
            .transpose()
    }

    #[cfg(test)]
    pub fn save_snapshot(&self, snapshot: &ProviderSnapshot) -> Result<(), StorageError> {
        self.save_snapshot_for_identity(snapshot, None)
    }

    pub fn save_snapshot_for_identity(
        &self,
        snapshot: &ProviderSnapshot,
        identity_key: Option<&str>,
    ) -> Result<(), StorageError> {
        let payload = serde_json::to_string(snapshot)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO provider_snapshots(provider_id, payload, refreshed_at, identity_key)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id) DO UPDATE SET
               payload = excluded.payload,
               refreshed_at = excluded.refreshed_at,
               identity_key = excluded.identity_key",
            params![
                snapshot.provider_id,
                payload,
                snapshot.refreshed_at.to_rfc3339(),
                identity_key,
            ],
        )?;
        transaction.execute(
            "DELETE FROM daily_usage WHERE provider_id = ?1",
            [&snapshot.provider_id],
        )?;
        for day in &snapshot.usage.daily {
            Self::insert_day(&transaction, &snapshot.provider_id, day)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_log_events(
        &self,
        provider_id: &str,
        path: &Path,
        size: u64,
        modified_nanos: i64,
    ) -> Result<Option<String>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT events_json FROM log_file_cache
                 WHERE provider_id = ?1 AND path = ?2 AND size = ?3 AND modified_nanos = ?4",
                params![
                    provider_id,
                    path.to_string_lossy(),
                    size as i64,
                    modified_nanos
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn save_log_events(
        &self,
        provider_id: &str,
        path: &Path,
        size: u64,
        modified_nanos: i64,
        events_json: &str,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO log_file_cache(path, size, modified_nanos, events_json, provider_id)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(provider_id, path) DO UPDATE SET
               size = excluded.size,
               modified_nanos = excluded.modified_nanos,
               events_json = excluded.events_json",
            params![
                path.to_string_lossy(),
                size as i64,
                modified_nanos,
                events_json,
                provider_id
            ],
        )?;
        Ok(())
    }

    pub fn remove_log_events(&self, provider_id: &str, path: &Path) -> Result<(), StorageError> {
        self.connection()?.execute(
            "DELETE FROM log_file_cache WHERE provider_id = ?1 AND path = ?2",
            params![provider_id, path.to_string_lossy()],
        )?;
        Ok(())
    }

    pub fn prune_log_events(
        &self,
        provider_id: &str,
        seen_paths: &HashSet<PathBuf>,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let cached_paths = {
            let mut statement =
                transaction.prepare("SELECT path FROM log_file_cache WHERE provider_id = ?1")?;
            let paths = statement
                .query_map([provider_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            paths
        };
        for cached_path in cached_paths {
            if !seen_paths.contains(Path::new(&cached_path)) {
                transaction.execute(
                    "DELETE FROM log_file_cache WHERE provider_id = ?1 AND path = ?2",
                    params![provider_id, cached_path],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_settings(&self) -> Result<Option<AppSettings>, StorageError> {
        let connection = self.connection()?;
        let payload: Option<String> = connection
            .query_row("SELECT payload FROM app_settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        payload
            .map(|json| serde_json::from_str(&json).map_err(StorageError::from))
            .transpose()
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        self.save_settings_with_account_updates(settings, &[])
    }

    pub fn save_settings_with_account_updates(
        &self,
        settings: &AppSettings,
        account_updates: &[ProviderAccountUpdate],
    ) -> Result<(), StorageError> {
        let payload = serde_json::to_string(settings)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for update in account_updates {
            transaction.execute(
                "INSERT INTO provider_account_records(
                   provider_family, identity_key, provider_id, payload
                 ) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(provider_family, identity_key) DO UPDATE SET
                   provider_id = excluded.provider_id,
                   payload = excluded.payload",
                params![
                    update.provider_family,
                    update.identity_key,
                    update.provider_id,
                    update.payload,
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO app_settings(id, payload) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            [payload],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_provider_environment(
        &self,
    ) -> Result<Option<HashMap<String, String>>, StorageError> {
        let connection = self.connection()?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM provider_environment WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(payload.and_then(|json| serde_json::from_str(&json).ok()))
    }

    #[cfg(unix)]
    pub fn save_provider_environment(
        &self,
        environment: &HashMap<String, String>,
    ) -> Result<(), StorageError> {
        let payload = serde_json::to_string(environment)?;
        self.connection()?.execute(
            "INSERT INTO provider_environment(id, payload) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            [payload],
        )?;
        Ok(())
    }

    pub fn load_provider_account_records(
        &self,
        provider_family: &str,
    ) -> Result<Vec<(String, String, String)>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT identity_key, provider_id, payload
             FROM provider_account_records
             WHERE provider_family = ?1
             ORDER BY provider_id",
        )?;
        let records = statement
            .query_map([provider_family], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        Ok(records)
    }

    pub fn load_observed_account_provider_ids(&self) -> Result<Vec<String>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT provider_id
             FROM provider_account_records
             ORDER BY provider_id",
        )?;
        let provider_ids = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        Ok(provider_ids)
    }

    pub fn save_provider_account_record(
        &self,
        provider_family: &str,
        identity_key: &str,
        provider_id: &str,
        payload: &str,
    ) -> Result<(), StorageError> {
        self.connection()?.execute(
            "INSERT INTO provider_account_records(
               provider_family, identity_key, provider_id, payload
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_family, identity_key) DO UPDATE SET
               provider_id = excluded.provider_id,
               payload = excluded.payload",
            params![provider_family, identity_key, provider_id, payload],
        )?;
        Ok(())
    }

    pub fn load_panel_height(&self) -> Result<Option<u32>, StorageError> {
        let connection = self.connection()?;
        let height = connection
            .query_row("SELECT height FROM panel_state WHERE id = 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?;
        Ok(height.and_then(|value| u32::try_from(value).ok()))
    }

    pub fn save_panel_height(&self, height: u32) -> Result<(), StorageError> {
        self.connection()?.execute(
            "INSERT INTO panel_state(id, height) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET height = excluded.height",
            [i64::from(height)],
        )?;
        Ok(())
    }

    pub fn clear_panel_height(&self) -> Result<(), StorageError> {
        self.connection()?
            .execute("DELETE FROM panel_state WHERE id = 1", [])?;
        Ok(())
    }

    fn insert_day(
        transaction: &rusqlite::Transaction<'_>,
        provider_id: &str,
        day: &DailyUsage,
    ) -> Result<(), rusqlite::Error> {
        transaction.execute(
            "INSERT INTO daily_usage(
               provider_id, date, tokens, estimated_cost_usd, estimate_complete
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                provider_id,
                day.date,
                day.tokens as i64,
                day.estimated_cost_usd,
                day.estimate_complete as i64
            ],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection.lock().map_err(|_| StorageError::Poisoned)
    }

    fn has_column(
        connection: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, rusqlite::Error> {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(columns.iter().any(|name| name == column))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use chrono::Utc;
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{ProviderAccountUpdate, Storage};
    use crate::models::{
        AppSettings, DailyUsage, ModelUsageBreakdown, ModelUsageEntry, ProviderSnapshot,
        QuotaFormat, QuotaWindow, UsageHistory, UsagePeriod,
    };
    use crate::providers::CacheIdentity;

    #[test]
    fn snapshot_round_trip_contains_no_credentials() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: Some("Plus".into()),
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory {
                today: Some(UsagePeriod {
                    tokens: 42,
                    estimated_cost_usd: Some(0.12),
                    cost_estimated: true,
                    estimate_complete: true,
                    model_breakdown: Some(ModelUsageBreakdown {
                        models: vec![ModelUsageEntry {
                            model: "gpt-5.4".into(),
                            total_tokens: 42,
                            cost_usd: Some(0.12),
                            variants: None,
                        }],
                        source_note: "From your Codex logs (estimated)".into(),
                    }),
                    unknown_models: Vec::new(),
                }),
                daily: vec![DailyUsage {
                    date: "2026-07-10".into(),
                    tokens: 42,
                    estimated_cost_usd: Some(0.12),
                    estimate_complete: true,
                }],
                ..UsageHistory::default()
            },
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };

        storage.save_snapshot(&snapshot).unwrap();

        assert_eq!(storage.load_snapshot("codex").unwrap(), Some(snapshot));
        let bytes = std::fs::read(directory.path().join("openquota.db")).unwrap();
        let database = String::from_utf8_lossy(&bytes);
        assert!(!database.contains("access_token"));
        assert!(!database.contains("refresh_token"));
    }

    #[test]
    fn account_scoped_snapshot_is_visible_only_to_the_same_identity() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "claude".into(),
            plan: Some("Max".into()),
            quotas: Vec::new(),
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };

        storage
            .save_snapshot_for_identity(&snapshot, Some("identity-a"))
            .unwrap();

        assert_eq!(
            storage
                .load_snapshot_for_identity("claude", CacheIdentity::Resolved("identity-a"))
                .unwrap(),
            Some(snapshot.clone())
        );
        assert!(storage
            .load_snapshot_for_identity("claude", CacheIdentity::Resolved("identity-b"))
            .unwrap()
            .is_none());
        assert!(storage.load_snapshot("claude").unwrap().is_none());
        assert_eq!(
            storage
                .load_snapshot_for_identity("claude", CacheIdentity::Unresolved)
                .unwrap(),
            Some(snapshot)
        );
    }

    #[test]
    fn provider_account_records_round_trip_by_family() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();

        storage
            .save_provider_account_record(
                "claude",
                "identity-a",
                "claude@12345678",
                r#"{"displayName":"Claude — Work"}"#,
            )
            .unwrap();

        assert_eq!(
            storage.load_provider_account_records("claude").unwrap(),
            [(
                "identity-a".to_owned(),
                "claude@12345678".to_owned(),
                r#"{"displayName":"Claude — Work"}"#.to_owned(),
            )]
        );
        assert!(storage
            .load_provider_account_records("codex")
            .unwrap()
            .is_empty());
        assert_eq!(
            storage.load_observed_account_provider_ids().unwrap(),
            ["claude@12345678"]
        );
    }

    #[test]
    fn account_records_and_settings_roll_back_together() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let original = AppSettings {
            theme: crate::models::ThemePreference::Light,
            ..AppSettings::default()
        };
        storage.save_settings(&original).unwrap();
        storage
            .save_provider_account_record("codex", "identity-a", "codex", r#"{"name":"old"}"#)
            .unwrap();
        let changed = AppSettings {
            theme: crate::models::ThemePreference::Dark,
            ..original.clone()
        };
        let updates = [
            ProviderAccountUpdate {
                provider_family: "codex".into(),
                identity_key: "identity-a".into(),
                provider_id: "codex".into(),
                payload: r#"{"name":"changed"}"#.into(),
            },
            ProviderAccountUpdate {
                provider_family: "codex".into(),
                identity_key: "identity-b".into(),
                provider_id: "codex".into(),
                payload: "{}".into(),
            },
        ];

        assert!(storage
            .save_settings_with_account_updates(&changed, &updates)
            .is_err());
        assert_eq!(storage.load_settings().unwrap(), Some(original));
        let records = storage.load_provider_account_records("codex").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].2, r#"{"name":"old"}"#);
    }

    #[test]
    fn legacy_snapshot_table_gains_the_account_identity_column() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("openquota.db");
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE provider_snapshots (
                   provider_id TEXT PRIMARY KEY,
                   payload TEXT NOT NULL,
                   refreshed_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        drop(legacy);

        let storage = Storage::open(&path).unwrap();
        let connection = storage.connection().unwrap();

        assert!(Storage::has_column(&connection, "provider_snapshots", "identity_key").unwrap());
    }

    #[test]
    fn legacy_count_quota_cache_is_normalized_at_load_boundary() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "cursor".into(),
            plan: None,
            quotas: vec![QuotaWindow {
                id: "requests".into(),
                label: "Requests".into(),
                used_percent: 25.0,
                resets_at: None,
                period_seconds: 2_592_000,
                format: QuotaFormat::Count,
                used_value: Some(25.0),
                limit_value: Some(100.0),
                unit: None,
                estimated: false,
                source_note: None,
            }],
            value_metrics: Vec::new(),
            status_metrics: Vec::new(),
            notices: Vec::new(),
            usage: UsageHistory::default(),
            warnings: Vec::new(),
            refreshed_at: Utc::now(),
        };

        storage.save_snapshot(&snapshot).unwrap();

        assert_eq!(
            storage.load_snapshot("cursor").unwrap().unwrap().quotas[0].unit,
            Some("requests".into())
        );
    }

    #[test]
    fn settings_round_trip_uses_the_same_disk_database() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let settings = AppSettings {
            always_show_pacing: true,
            ..AppSettings::default()
        };
        storage.save_settings(&settings).unwrap();
        assert_eq!(storage.load_settings().unwrap(), Some(settings));
    }

    #[test]
    fn panel_height_round_trip_is_independent_from_app_settings() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let settings = AppSettings::default();
        storage.save_settings(&settings).unwrap();

        assert_eq!(storage.load_panel_height().unwrap(), None);
        storage.save_panel_height(734).unwrap();

        assert_eq!(storage.load_panel_height().unwrap(), Some(734));
        assert_eq!(storage.load_settings().unwrap(), Some(settings));

        storage.clear_panel_height().unwrap();
        assert_eq!(storage.load_panel_height().unwrap(), None);
    }

    #[test]
    fn log_cache_pruning_is_scoped_to_a_provider() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let codex_old = PathBuf::from("/logs/codex-old.jsonl");
        let codex_current = PathBuf::from("/logs/codex-current.jsonl");
        let claude_current = PathBuf::from("/logs/claude-current.jsonl");
        for (provider, path) in [
            ("codex", &codex_old),
            ("codex", &codex_current),
            ("claude", &claude_current),
        ] {
            storage
                .save_log_events(provider, path, 10, 20, "[]")
                .unwrap();
        }

        storage
            .prune_log_events("codex", &HashSet::from([codex_current.clone()]))
            .unwrap();

        assert_eq!(
            storage
                .load_log_events("codex", &codex_old, 10, 20)
                .unwrap(),
            None
        );
        assert_eq!(
            storage
                .load_log_events("codex", &codex_current, 10, 20)
                .unwrap(),
            Some("[]".to_owned())
        );
        assert_eq!(
            storage
                .load_log_events("claude", &claude_current, 10, 20)
                .unwrap(),
            Some("[]".to_owned())
        );
    }

    #[test]
    fn providers_can_cache_the_same_path_independently() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let shared = PathBuf::from("/synced/session.jsonl");
        storage
            .save_log_events("claude", &shared, 10, 20, "claude")
            .unwrap();
        storage
            .save_log_events("codex", &shared, 10, 20, "codex")
            .unwrap();

        assert_eq!(
            storage.load_log_events("claude", &shared, 10, 20).unwrap(),
            Some("claude".to_owned())
        );
        assert_eq!(
            storage.load_log_events("codex", &shared, 10, 20).unwrap(),
            Some("codex".to_owned())
        );
    }

    #[test]
    fn legacy_millisecond_log_cache_is_safely_rebuilt() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("openquota.db");
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE log_file_cache (
                   path TEXT PRIMARY KEY,
                   size INTEGER NOT NULL,
                   modified_millis INTEGER NOT NULL,
                   events_json TEXT NOT NULL,
                   provider_id TEXT NOT NULL DEFAULT ''
                 );
                 INSERT INTO log_file_cache VALUES ('old.jsonl', 10, 20, '[]', 'codex');",
            )
            .unwrap();
        drop(legacy);

        let storage = Storage::open(&path).unwrap();
        assert_eq!(
            storage
                .load_log_events("codex", PathBuf::from("old.jsonl").as_path(), 10, 20)
                .unwrap(),
            None
        );
        storage
            .save_log_events("codex", PathBuf::from("new.jsonl").as_path(), 10, 20, "[]")
            .unwrap();
    }
}
