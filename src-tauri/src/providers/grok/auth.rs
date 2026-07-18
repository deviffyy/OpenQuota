use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile::NamedTempFile;

use super::GrokError;

const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const REFRESH_BUFFER_MINUTES: i64 = 5;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GrokAuthEntry {
    pub key: Option<String>,
    #[serde(rename = "refresh_token")]
    pub refresh_token: Option<String>,
    pub refresh: Option<String>,
    #[serde(rename = "id_token")]
    pub id_token: Option<String>,
    #[serde(rename = "expires_at")]
    pub expires_at: Option<String>,
    pub expires: Option<String>,
    #[serde(rename = "oidc_client_id")]
    pub oidc_client_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GrokAuthState {
    document: Value,
    pub entry_key: String,
    pub entry: GrokAuthEntry,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct GrokAuthStore {
    path: PathBuf,
}

impl GrokAuthStore {
    pub fn new() -> Self {
        Self {
            path: home_directory().join(".grok").join("auth.json"),
        }
    }

    #[cfg(test)]
    pub fn for_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn has_local_credentials(&self) -> bool {
        self.load_candidates()
            .is_ok_and(|candidates| !candidates.is_empty())
    }

    pub fn load_candidates(&self) -> Result<Vec<GrokAuthState>, GrokError> {
        if !self.path.is_file() {
            return Err(GrokError::NotLoggedIn);
        }
        let text = fs::read_to_string(&self.path).map_err(|_| GrokError::InvalidAuth)?;
        let document = parse_auth_document(&text).ok_or(GrokError::InvalidAuth)?;
        let entries = document.as_object().ok_or(GrokError::InvalidAuth)?;
        let candidates = entries
            .iter()
            .filter_map(|(entry_key, value)| {
                let entry = serde_json::from_value::<GrokAuthEntry>(value.clone()).ok()?;
                let token = trimmed(entry.key.as_deref())?.to_owned();
                Some(GrokAuthState {
                    document: document.clone(),
                    entry_key: entry_key.clone(),
                    entry,
                    token,
                })
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            Err(GrokError::InvalidAuth)
        } else {
            Ok(candidates)
        }
    }

    pub fn needs_refresh(&self, state: &GrokAuthState, now: DateTime<Utc>) -> bool {
        let threshold = now + Duration::minutes(REFRESH_BUFFER_MINUTES);
        entry_expiry(&state.entry).is_some_and(|expiry| expiry <= threshold)
            || token_expiry(&state.token).is_some_and(|expiry| expiry <= threshold)
    }

    pub fn is_expired(&self, state: &GrokAuthState, now: DateTime<Utc>) -> bool {
        token_expiry(&state.token)
            .or_else(|| entry_expiry(&state.entry))
            .is_some_and(|expiry| now >= expiry)
    }

    pub fn refresh_token<'a>(&self, state: &'a GrokAuthState) -> Option<&'a str> {
        trimmed(state.entry.refresh_token.as_deref())
            .or_else(|| trimmed(state.entry.refresh.as_deref()))
    }

    pub fn client_id(&self, state: &GrokAuthState) -> String {
        if let Some(value) = trimmed(state.entry.oidc_client_id.as_deref()) {
            return value.to_owned();
        }
        state
            .entry_key
            .rsplit_once("::")
            .map(|(_, suffix)| suffix)
            .and_then(|value| trimmed(Some(value)))
            .unwrap_or(DEFAULT_CLIENT_ID)
            .to_owned()
    }

    pub fn update_from_refresh(
        &self,
        state: &mut GrokAuthState,
        access_token: String,
        refresh_token: Option<String>,
        id_token: Option<String>,
        expires_in: Option<f64>,
        now: DateTime<Utc>,
    ) {
        state.token.clone_from(&access_token);
        state.entry.key = Some(access_token);
        if let Some(value) = refresh_token.and_then(trimmed_owned) {
            state.entry.refresh_token = Some(value);
        }
        if let Some(value) = id_token.and_then(trimmed_owned) {
            state.entry.id_token = Some(value);
        }
        let expiry = expires_in
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|seconds| now + Duration::milliseconds((seconds * 1_000.0).round() as i64))
            .or_else(|| token_expiry(&state.token))
            .unwrap_or_else(|| now + Duration::hours(1));
        state.entry.expires_at = Some(expiry.to_rfc3339_opts(SecondsFormat::Millis, true));
    }

    /// Re-reads the current document before replacing it so another account and unknown fields are
    /// retained. A present but unreadable file is never rebuilt from stale in-memory state.
    pub fn save(&self, state: &mut GrokAuthState) -> Result<(), GrokError> {
        let mut document = if self.path.exists() {
            let text = fs::read_to_string(&self.path).map_err(|_| GrokError::InvalidAuth)?;
            parse_auth_document(&text).ok_or(GrokError::InvalidAuth)?
        } else {
            state.document.clone()
        };
        let entries = document.as_object_mut().ok_or(GrokError::InvalidAuth)?;
        let mut entry = entries
            .get(&state.entry_key)
            .and_then(Value::as_object)
            .cloned()
            .or_else(|| {
                state
                    .document
                    .get(&state.entry_key)
                    .and_then(Value::as_object)
                    .cloned()
            })
            .unwrap_or_default();
        set_optional_string(&mut entry, "key", state.entry.key.as_deref());
        set_optional_string(
            &mut entry,
            "refresh_token",
            state.entry.refresh_token.as_deref(),
        );
        set_optional_string(&mut entry, "id_token", state.entry.id_token.as_deref());
        set_optional_string(&mut entry, "expires_at", state.entry.expires_at.as_deref());
        entries.insert(state.entry_key.clone(), Value::Object(entry));

        write_private_json_atomic(&self.path, &document)?;
        state.document = document;
        Ok(())
    }
}

fn set_optional_string(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn write_private_json_atomic(path: &Path, document: &Value) -> Result<(), GrokError> {
    let parent = path.parent().ok_or(GrokError::AuthWrite)?;
    fs::create_dir_all(parent).map_err(|_| GrokError::AuthWrite)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|_| GrokError::AuthWrite)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| GrokError::AuthWrite)?;
    }
    serde_json::to_writer_pretty(&mut temporary, document).map_err(|_| GrokError::AuthWrite)?;
    temporary
        .write_all(b"\n")
        .map_err(|_| GrokError::AuthWrite)?;
    temporary.flush().map_err(|_| GrokError::AuthWrite)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| GrokError::AuthWrite)?;
    temporary.persist(path).map_err(|_| GrokError::AuthWrite)?;
    Ok(())
}

fn parse_auth_document(text: &str) -> Option<Value> {
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
}

fn entry_expiry(entry: &GrokAuthEntry) -> Option<DateTime<Utc>> {
    trimmed(entry.expires_at.as_deref())
        .or_else(|| trimmed(entry.expires.as_deref()))
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.to_utc())
}

pub fn token_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let seconds = value
        .get("exp")
        .and_then(number)
        .filter(|value| value.is_finite())?;
    DateTime::from_timestamp(seconds as i64, 0)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn trimmed_owned(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{token_expiry, GrokAuthStore, DEFAULT_CLIENT_ID};
    use crate::providers::grok::GrokError;

    fn jwt(expiry: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"exp": expiry})).unwrap());
        format!("header.{payload}.signature")
    }

    #[test]
    fn loads_all_keyed_accounts_and_derives_client_ids() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        fs::write(
            &path,
            r#"{
              "https://auth.x.ai::client-a":{"key":" token-a ","refresh_token":"refresh-a"},
              "https://auth.x.ai::client-b":{"key":"token-b"},
              "empty":{"refresh_token":"refresh-only"}
            }"#,
        )
        .unwrap();
        let store = GrokAuthStore::for_path(path);

        let candidates = store.load_candidates().unwrap();

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].token, "token-a");
        assert_eq!(store.client_id(&candidates[0]), "client-a");
        assert_eq!(store.client_id(&candidates[1]), "client-b");
    }

    #[test]
    fn jwt_and_entry_expiry_drive_the_refresh_window() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "account": {
                    "key": jwt((now + Duration::minutes(2)).timestamp()),
                    "expires_at": (now + Duration::days(1)).to_rfc3339()
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let store = GrokAuthStore::for_path(path);
        let state = store.load_candidates().unwrap().remove(0);

        assert_eq!(
            token_expiry(&state.token).unwrap().timestamp(),
            (now + Duration::minutes(2)).timestamp()
        );
        assert!(store.needs_refresh(&state, now));
        assert!(!store.is_expired(&state, now));
        assert_eq!(store.client_id(&state), DEFAULT_CLIENT_ID);
    }

    #[test]
    fn save_preserves_unknown_fields_and_other_accounts() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        fs::write(
            &path,
            r#"{
              "https://auth.x.ai::client":{
                "key":"old","refresh_token":"old-refresh","custom_field":"keep"
              },
              "other":{"key":"other-token","nested":{"keep":true}}
            }"#,
        )
        .unwrap();
        let store = GrokAuthStore::for_path(path.clone());
        let mut state = store.load_candidates().unwrap().remove(0);
        let now = Utc.with_ymd_and_hms(2026, 2, 2, 0, 0, 0).unwrap();
        store.update_from_refresh(
            &mut state,
            "new-token".into(),
            Some("new-refresh".into()),
            None,
            Some(3600.0),
            now,
        );

        store.save(&mut state).unwrap();

        let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["https://auth.x.ai::client"]["custom_field"], "keep");
        assert_eq!(saved["https://auth.x.ai::client"]["key"], "new-token");
        assert_eq!(saved["other"]["nested"]["keep"], true);
    }

    #[test]
    fn save_refuses_to_replace_a_corrupt_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        fs::write(&path, r#"{"account":{"key":"old"}}"#).unwrap();
        let store = GrokAuthStore::for_path(path.clone());
        let mut state = store.load_candidates().unwrap().remove(0);
        state.entry.key = Some("new-secret".into());
        state.token = "new-secret".into();
        fs::write(&path, "{ corrupt").unwrap();

        let error = store.save(&mut state).unwrap_err();

        assert!(matches!(error, GrokError::InvalidAuth));
        assert_eq!(fs::read_to_string(path).unwrap(), "{ corrupt");
        assert!(!error.to_string().contains("new-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn persisted_credentials_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        fs::write(&path, r#"{"account":{"key":"old"}}"#).unwrap();
        let store = GrokAuthStore::for_path(path.clone());
        let mut state = store.load_candidates().unwrap().remove(0);

        store.save(&mut state).unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
