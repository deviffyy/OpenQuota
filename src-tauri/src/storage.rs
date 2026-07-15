use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use crate::models::{AppSettings, DailyUsage, ProviderSnapshot};

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
               refreshed_at TEXT NOT NULL
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
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn load_snapshot(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderSnapshot>, StorageError> {
        let connection = self.connection()?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM provider_snapshots WHERE provider_id = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|json| serde_json::from_str(&json).map_err(StorageError::from))
            .transpose()
    }

    pub fn save_snapshot(&self, snapshot: &ProviderSnapshot) -> Result<(), StorageError> {
        let payload = serde_json::to_string(snapshot)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO provider_snapshots(provider_id, payload, refreshed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(provider_id) DO UPDATE SET
               payload = excluded.payload,
               refreshed_at = excluded.refreshed_at",
            params![
                snapshot.provider_id,
                payload,
                snapshot.refreshed_at.to_rfc3339()
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
        let payload = serde_json::to_string(settings)?;
        self.connection()?.execute(
            "INSERT INTO app_settings(id, payload) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            [payload],
        )?;
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

    use super::Storage;
    use crate::models::{
        AppSettings, DailyUsage, ModelUsageBreakdown, ModelUsageEntry, ProviderSnapshot,
        UsageHistory, UsagePeriod,
    };

    #[test]
    fn snapshot_round_trip_contains_no_credentials() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: Some("Plus".into()),
            quotas: Vec::new(),
            value_metrics: Vec::new(),
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
