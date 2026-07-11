use std::{
    fs,
    path::Path,
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
               path TEXT PRIMARY KEY,
               size INTEGER NOT NULL,
               modified_millis INTEGER NOT NULL,
               events_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS app_settings (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               payload TEXT NOT NULL
             );",
        )?;
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
        path: &Path,
        size: u64,
        modified_millis: i64,
    ) -> Result<Option<String>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT events_json FROM log_file_cache
                 WHERE path = ?1 AND size = ?2 AND modified_millis = ?3",
                params![path.to_string_lossy(), size as i64, modified_millis],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn save_log_events(
        &self,
        path: &Path,
        size: u64,
        modified_millis: i64,
        events_json: &str,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO log_file_cache(path, size, modified_millis, events_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
               size = excluded.size,
               modified_millis = excluded.modified_millis,
               events_json = excluded.events_json",
            params![
                path.to_string_lossy(),
                size as i64,
                modified_millis,
                events_json
            ],
        )?;
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
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::Storage;
    use crate::models::{AppSettings, DailyUsage, ProviderSnapshot, UsageHistory};

    #[test]
    fn snapshot_round_trip_contains_no_credentials() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let snapshot = ProviderSnapshot {
            provider_id: "codex".into(),
            plan: Some("Plus".into()),
            quotas: Vec::new(),
            usage: UsageHistory {
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
}
