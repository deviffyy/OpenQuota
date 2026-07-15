use std::{
    fs::{self, Metadata},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::storage::{Storage, StorageError};

/// Cross-platform metadata key for one local log file. Nanosecond precision avoids stale cache hits
/// when sync tools or fast writers replace a same-sized file more than once inside one millisecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogFileFingerprint {
    pub size: u64,
    pub modified_nanos: i64,
}

impl LogFileFingerprint {
    pub fn from_metadata(metadata: &Metadata) -> Option<Self> {
        Some(Self {
            size: metadata.len(),
            modified_nanos: system_time_nanos(metadata.modified().ok()?),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogCacheError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("parsed local log events could not be cached")]
    Encode(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct CachedLogEvents<T> {
    schema_version: u8,
    events: Vec<T>,
}

#[derive(Serialize)]
struct CachedLogEventsRef<'a, T> {
    schema_version: u8,
    events: &'a [T],
}

/// Loads one parsed log file from the persistent cache or refreshes it from disk. An unreadable file
/// is deliberately not an error for the whole provider and never becomes a cached empty result; its
/// stale cache row is removed so a later refresh retries the source file.
pub fn load_or_parse_log<T>(
    storage: &Storage,
    provider_id: &str,
    path: &Path,
    schema_version: u8,
    parse: impl FnOnce(&str) -> Vec<T>,
) -> Result<Option<Vec<T>>, LogCacheError>
where
    T: Serialize + DeserializeOwned,
{
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            storage.remove_log_events(provider_id, path)?;
            return Ok(None);
        }
    };
    let Some(fingerprint) = LogFileFingerprint::from_metadata(&metadata) else {
        storage.remove_log_events(provider_id, path)?;
        return Ok(None);
    };
    if let Some(events) = storage
        .load_log_events(
            provider_id,
            path,
            fingerprint.size,
            fingerprint.modified_nanos,
        )?
        .as_deref()
        .and_then(|json| serde_json::from_str::<CachedLogEvents<T>>(json).ok())
        .filter(|cached| cached.schema_version == schema_version)
        .map(|cached| cached.events)
    {
        return Ok(Some(events));
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            storage.remove_log_events(provider_id, path)?;
            return Ok(None);
        }
    };
    let events = parse(&content);
    let json = serde_json::to_string(&CachedLogEventsRef {
        schema_version,
        events: &events,
    })?;
    storage.save_log_events(
        provider_id,
        path,
        fingerprint.size,
        fingerprint.modified_nanos,
        &json,
    )?;
    Ok(Some(events))
}

fn system_time_nanos(value: SystemTime) -> i64 {
    match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => (duration.as_secs() as i64)
            .saturating_mul(1_000_000_000)
            .saturating_add(i64::from(duration.subsec_nanos())),
        Err(error) => {
            let duration = error.duration();
            (duration.as_secs() as i64)
                .saturating_mul(1_000_000_000)
                .saturating_add(i64::from(duration.subsec_nanos()))
                .saturating_neg()
        }
    }
}

/// Parses the timestamp variants emitted by provider CLIs on different operating systems and sync
/// paths. Calendar bucketing remains device-local after this returns a single absolute UTC instant.
pub fn parse_log_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let mut value = raw.trim().to_owned();
    if value.is_empty() {
        return None;
    }
    if value.ends_with(" UTC") {
        value.truncate(value.len() - 4);
        value.push('Z');
    }
    if value.as_bytes().get(10) == Some(&b' ') {
        value.replace_range(10..11, "T");
    }
    normalize_fraction(&mut value);
    if value.len() >= 19 && !has_timezone(&value) {
        value.push('Z');
    }
    DateTime::parse_from_rfc3339(&value)
        .ok()
        .map(|timestamp| timestamp.to_utc())
}

fn normalize_fraction(value: &mut String) {
    if value.as_bytes().get(19) != Some(&b'.') {
        return;
    }
    let fraction_end = value[20..]
        .find(['Z', '+', '-'])
        .map(|index| index + 20)
        .unwrap_or(value.len());
    if fraction_end == 20
        || !value.as_bytes()[20..fraction_end]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return;
    }
    let digits = fraction_end - 20;
    if digits > 3 {
        value.replace_range(23..fraction_end, "");
    } else if digits < 3 {
        value.insert_str(fraction_end, &"0".repeat(3 - digits));
    }
}

fn has_timezone(value: &str) -> bool {
    let suffix = value.get(19..).unwrap_or_default();
    suffix.ends_with('Z') || suffix.rfind(['+', '-']).is_some()
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs};

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::{load_or_parse_log, parse_log_timestamp, LogFileFingerprint};
    use crate::storage::Storage;

    #[test]
    fn timestamp_variants_resolve_to_the_same_absolute_instant() {
        let expected = Utc.with_ymd_and_hms(2026, 7, 15, 9, 30, 45).unwrap()
            + chrono::Duration::milliseconds(123);
        for value in [
            "2026-07-15T09:30:45.123Z",
            " 2026-07-15 09:30:45.123456 UTC ",
            "2026-07-15T09:30:45.123",
            "2026-07-15T12:30:45.123+03:00",
            "2026-07-15T02:30:45.123-07:00",
        ] {
            assert_eq!(parse_log_timestamp(value), Some(expected), "value: {value}");
        }
    }

    #[test]
    fn timestamp_parser_rejects_non_timestamp_text() {
        assert_eq!(parse_log_timestamp(""), None);
        assert_eq!(parse_log_timestamp("tomorrow"), None);
    }

    #[test]
    fn unchanged_files_reuse_cache_and_schema_changes_reparse() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let path = directory.path().join("session.jsonl");
        fs::write(&path, "first").unwrap();
        let parses = Cell::new(0);
        let parse = |content: &str| {
            parses.set(parses.get() + 1);
            vec![content.to_owned()]
        };

        assert_eq!(
            load_or_parse_log(&storage, "test", &path, 1, parse).unwrap(),
            Some(vec!["first".to_owned()])
        );
        assert_eq!(
            load_or_parse_log(&storage, "test", &path, 1, parse).unwrap(),
            Some(vec!["first".to_owned()])
        );
        assert_eq!(parses.get(), 1);
        load_or_parse_log(&storage, "test", &path, 2, parse).unwrap();
        assert_eq!(parses.get(), 2);
    }

    #[test]
    fn unreadable_text_removes_stale_cache_and_retries_later() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let path = directory.path().join("session.jsonl");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let fingerprint = LogFileFingerprint::from_metadata(&metadata).unwrap();
        storage
            .save_log_events(
                "test",
                &path,
                fingerprint.size,
                fingerprint.modified_nanos.saturating_sub(1),
                r#"{"schema_version":1,"events":["stale"]}"#,
            )
            .unwrap();

        assert_eq!(
            load_or_parse_log::<String>(&storage, "test", &path, 1, |_| Vec::new()).unwrap(),
            None
        );
        assert_eq!(
            storage
                .load_log_events(
                    "test",
                    &path,
                    fingerprint.size,
                    fingerprint.modified_nanos.saturating_sub(1),
                )
                .unwrap(),
            None
        );

        fs::write(&path, "recovered").unwrap();
        assert_eq!(
            load_or_parse_log(&storage, "test", &path, 1, |content| {
                vec![content.to_owned()]
            })
            .unwrap(),
            Some(vec!["recovered".to_owned()])
        );
    }
}
