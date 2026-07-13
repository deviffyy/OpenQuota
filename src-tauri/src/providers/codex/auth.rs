use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tempfile::NamedTempFile;

use super::CodexError;

const REFRESH_WINDOW: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct CodexAuthState {
    source: AuthSource,
    pub document: Value,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
    pub last_refresh: Option<String>,
}

#[derive(Debug, Clone)]
enum AuthSource {
    File(PathBuf),
    #[cfg(target_os = "macos")]
    Keychain,
}

impl CodexAuthState {
    pub fn has_local_credentials() -> bool {
        let file_credentials = auth_paths().into_iter().any(|path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|text| parse_auth_document(&text))
                .is_some_and(|document| auth_document_has_credentials(&document))
        });
        file_credentials
            || keychain_document().is_some_and(|document| auth_document_has_credentials(&document))
    }

    pub fn load() -> Result<Self, CodexError> {
        let mut api_key_only = false;
        for path in auth_paths() {
            if !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(|_| CodexError::InvalidAuth)?;
            let document = parse_auth_document(&text).ok_or(CodexError::InvalidAuth)?;
            let access_token = document
                .pointer("/tokens/access_token")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if let Some(access_token) = access_token {
                return Ok(Self {
                    source: AuthSource::File(path),
                    refresh_token: string_at(&document, "/tokens/refresh_token"),
                    account_id: string_at(&document, "/tokens/account_id"),
                    last_refresh: string_at(&document, "/last_refresh"),
                    document,
                    access_token,
                });
            }
            api_key_only |= document
                .get("OPENAI_API_KEY")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty());
        }
        if let Some(state) = load_keychain_candidate() {
            return Ok(state);
        }
        if api_key_only {
            Err(CodexError::ApiKeyOnly)
        } else {
            Err(CodexError::NotLoggedIn)
        }
    }

    pub fn reload(&self) -> Result<Self, CodexError> {
        match &self.source {
            AuthSource::File(path) => load_from_path(path),
            #[cfg(target_os = "macos")]
            AuthSource::Keychain => load_from_keychain(),
        }
    }

    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        if let Some(expiry) = jwt_expiry(&self.access_token) {
            return expiry.signed_duration_since(now).num_seconds()
                <= REFRESH_WINDOW.as_secs() as i64;
        }
        self.last_refresh
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|date| now.signed_duration_since(date.to_utc()).num_days() > 8)
    }

    pub fn update_and_save(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        id_token: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), CodexError> {
        set_string(&mut self.document, "/tokens/access_token", &access_token)?;
        if let Some(value) = refresh_token.as_deref() {
            set_string(&mut self.document, "/tokens/refresh_token", value)?;
            self.refresh_token = Some(value.to_owned());
        }
        if let Some(value) = id_token.as_deref() {
            set_string(&mut self.document, "/tokens/id_token", value)?;
        }
        let refreshed_at = now.to_rfc3339();
        set_string(&mut self.document, "/last_refresh", &refreshed_at)?;
        self.access_token = access_token;
        self.last_refresh = Some(refreshed_at);

        match &self.source {
            AuthSource::File(path) => save_file_document(path, &self.document),
            #[cfg(target_os = "macos")]
            AuthSource::Keychain => save_keychain_document(&self.document),
        }
    }
}

fn load_from_path(path: &Path) -> Result<CodexAuthState, CodexError> {
    let text = fs::read_to_string(path).map_err(|_| CodexError::InvalidAuth)?;
    let document = parse_auth_document(&text).ok_or(CodexError::InvalidAuth)?;
    let access_token = string_at(&document, "/tokens/access_token")
        .filter(|value| !value.is_empty())
        .ok_or(CodexError::NotLoggedIn)?;
    Ok(CodexAuthState {
        source: AuthSource::File(path.to_path_buf()),
        refresh_token: string_at(&document, "/tokens/refresh_token"),
        account_id: string_at(&document, "/tokens/account_id"),
        last_refresh: string_at(&document, "/last_refresh"),
        document,
        access_token,
    })
}

fn save_file_document(path: &Path, document: &Value) -> Result<(), CodexError> {
    let parent = path.parent().ok_or(CodexError::InvalidAuth)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|_| CodexError::AuthWrite)?;
    serde_json::to_writer_pretty(&mut temporary, document).map_err(|_| CodexError::AuthWrite)?;
    temporary
        .write_all(b"\n")
        .map_err(|_| CodexError::AuthWrite)?;
    temporary.flush().map_err(|_| CodexError::AuthWrite)?;
    temporary.persist(path).map_err(|_| CodexError::AuthWrite)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn keychain_document() -> Option<Value> {
    use security_framework::passwords::{generic_password, PasswordOptions};

    let bytes = generic_password(PasswordOptions::new_generic_password("Codex Auth", "")).ok()?;
    parse_auth_document(std::str::from_utf8(&bytes).ok()?)
}

#[cfg(target_os = "macos")]
fn load_keychain_candidate() -> Option<CodexAuthState> {
    let document = keychain_document()?;
    let access_token =
        string_at(&document, "/tokens/access_token").filter(|value| !value.is_empty())?;
    Some(CodexAuthState {
        source: AuthSource::Keychain,
        refresh_token: string_at(&document, "/tokens/refresh_token"),
        account_id: string_at(&document, "/tokens/account_id"),
        last_refresh: string_at(&document, "/last_refresh"),
        document,
        access_token,
    })
}

#[cfg(not(target_os = "macos"))]
fn load_keychain_candidate() -> Option<CodexAuthState> {
    None
}

#[cfg(not(target_os = "macos"))]
fn keychain_document() -> Option<Value> {
    None
}

#[cfg(target_os = "macos")]
fn load_from_keychain() -> Result<CodexAuthState, CodexError> {
    let document = keychain_document().ok_or(CodexError::NotLoggedIn)?;
    let access_token = string_at(&document, "/tokens/access_token")
        .filter(|value| !value.is_empty())
        .ok_or(CodexError::NotLoggedIn)?;
    Ok(CodexAuthState {
        source: AuthSource::Keychain,
        refresh_token: string_at(&document, "/tokens/refresh_token"),
        account_id: string_at(&document, "/tokens/account_id"),
        last_refresh: string_at(&document, "/last_refresh"),
        document,
        access_token,
    })
}

#[cfg(target_os = "macos")]
fn save_keychain_document(document: &Value) -> Result<(), CodexError> {
    use security_framework::passwords::set_generic_password;

    let bytes = serde_json::to_vec(document).map_err(|_| CodexError::AuthWrite)?;
    set_generic_password("Codex Auth", "", &bytes).map_err(|_| CodexError::AuthWrite)
}

pub fn auth_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    candidate_paths(
        &home,
        std::env::var_os("CODEX_HOME").map(PathBuf::from).as_deref(),
    )
}

fn candidate_paths(home: &Path, codex_home: Option<&Path>) -> Vec<PathBuf> {
    if let Some(codex_home) = codex_home.filter(|path| !path.as_os_str().is_empty()) {
        return vec![codex_home.join("auth.json")];
    }
    vec![
        home.join(".config").join("codex").join("auth.json"),
        home.join(".codex").join("auth.json"),
    ]
}

fn parse_auth_document(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok().or_else(|| {
        let trimmed = text.trim();
        if !trimmed.len().is_multiple_of(2) || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let bytes = (0..trimmed.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&trimmed[index..index + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        serde_json::from_slice(&bytes).ok()
    })
}

fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    DateTime::from_timestamp(value.get("exp")?.as_i64()?, 0)
}

fn string_at(document: &Value, pointer: &str) -> Option<String> {
    document
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn auth_document_has_credentials(document: &Value) -> bool {
    [
        "/tokens/access_token",
        "/tokens/refresh_token",
        "/OPENAI_API_KEY",
    ]
    .into_iter()
    .any(|pointer| {
        document
            .pointer(pointer)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    })
}

fn set_string(document: &mut Value, pointer: &str, value: &str) -> Result<(), CodexError> {
    let segments = pointer
        .split('/')
        .skip(1)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let (leaf, parents) = segments.split_last().ok_or(CodexError::InvalidAuth)?;
    let mut cursor = document;
    for segment in parents {
        let object = cursor.as_object_mut().ok_or(CodexError::InvalidAuth)?;
        cursor = object
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    cursor
        .as_object_mut()
        .ok_or(CodexError::InvalidAuth)?
        .insert((*leaf).to_owned(), Value::String(value.to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        auth_document_has_credentials, candidate_paths, parse_auth_document, AuthSource,
        CodexAuthState,
    };
    use crate::providers::codex::CodexError;

    #[test]
    fn codex_home_replaces_default_candidates() {
        assert_eq!(
            candidate_paths(Path::new("/users/me"), Some(Path::new("/custom/codex"))),
            vec![Path::new("/custom/codex/auth.json")]
        );
    }

    #[test]
    fn parses_hex_encoded_auth_without_exposing_tokens() {
        let raw = r#"{"tokens":{"access_token":"placeholder"}}"#;
        let hex = raw
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            parse_auth_document(&hex)
                .unwrap()
                .pointer("/tokens/access_token")
                .and_then(|value| value.as_str()),
            Some("placeholder")
        );
    }

    #[test]
    fn jwt_expiry_controls_refresh_window() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"exp": (now + Duration::minutes(1)).timestamp()})).unwrap(),
        );
        let state = CodexAuthState {
            source: super::AuthSource::File("auth.json".into()),
            document: json!({}),
            access_token: format!("header.{payload}.signature"),
            refresh_token: None,
            account_id: None,
            last_refresh: None,
        };
        assert!(state.needs_refresh(now));
    }

    #[test]
    fn local_detection_accepts_oauth_or_api_key_credentials() {
        assert!(auth_document_has_credentials(
            &json!({"tokens":{"access_token":"placeholder"}})
        ));
        assert!(auth_document_has_credentials(
            &json!({"OPENAI_API_KEY":"placeholder"})
        ));
        assert!(!auth_document_has_credentials(
            &json!({"tokens":{"access_token":""}})
        ));
    }

    #[test]
    fn credential_write_failures_are_typed_and_do_not_expose_tokens() {
        let directory = tempdir().unwrap();
        let blocked_parent = directory.path().join("not-a-directory");
        fs::write(&blocked_parent, b"block directory creation").unwrap();
        let mut state = CodexAuthState {
            source: AuthSource::File(blocked_parent.join("auth.json")),
            document: json!({"tokens": {}}),
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            account_id: None,
            last_refresh: None,
        };

        let error = state
            .update_and_save(
                "secret-access".into(),
                Some("secret-refresh".into()),
                None,
                Utc::now(),
            )
            .unwrap_err();

        assert!(matches!(error, CodexError::AuthWrite));
        assert!(!error.to_string().contains("secret-access"));
        assert!(!error.to_string().contains("secret-refresh"));
    }
}
