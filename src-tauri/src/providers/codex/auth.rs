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
    Hermes(PathBuf, String),
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

    pub fn load_candidates() -> Result<Vec<Self>, CodexError> {
        let mut candidates = Vec::new();
        let mut api_key_only = false;
        for path in auth_paths() {
            if !path.is_file() {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(document) = parse_auth_document(&text) else {
                continue;
            };
            let access_token = document
                .pointer("/tokens/access_token")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if let Some(access_token) = access_token {
                candidates.push(Self {
                    source: AuthSource::File(path),
                    refresh_token: string_at(&document, "/tokens/refresh_token"),
                    account_id: string_at(&document, "/tokens/account_id"),
                    last_refresh: string_at(&document, "/last_refresh"),
                    document,
                    access_token,
                });
                continue;
            }
            api_key_only |= document
                .get("OPENAI_API_KEY")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty());
        }
        if let Some(state) = load_keychain_candidate() {
            candidates.push(state);
        }
        if !candidates.is_empty() {
            Ok(candidates)
        } else if api_key_only {
            Err(CodexError::ApiKeyOnly)
        } else {
            Err(CodexError::NotLoggedIn)
        }
    }

    pub fn observed_account_identity() -> Option<String> {
        Self::load_candidates()
            .ok()?
            .into_iter()
            .find_map(|state| state.account_identity())
    }

    pub fn load_candidates_scoped(
        source: &super::accounts::CodexAuthSource,
    ) -> Result<Vec<Self>, CodexError> {
        match source {
            super::accounts::CodexAuthSource::Standard => Self::load_candidates(),
            super::accounts::CodexAuthSource::Home(path) => {
                let auth_path = path.join("auth.json");
                let state = load_from_path(&auth_path)?;
                Ok(vec![state])
            }
            super::accounts::CodexAuthSource::Hermes(path, hermes_id) => {
                let state = load_hermes_from_path(path, hermes_id)?;
                Ok(vec![state])
            }
        }
    }

    pub fn has_local_credentials_scoped(source: &super::accounts::CodexAuthSource) -> bool {
        Self::load_candidates_scoped(source).is_ok()
    }

    pub fn account_identity(&self) -> Option<String> {
        self.account_id
            .as_deref()
            .and_then(nonempty_lowercase)
            .or_else(|| {
                self.document
                    .pointer("/tokens/id_token")
                    .and_then(Value::as_str)
                    .and_then(jwt_payload)
                    .and_then(|payload| {
                        payload
                            .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                            .or_else(|| payload.get("chatgpt_account_id"))
                            .and_then(Value::as_str)
                            .and_then(nonempty_lowercase)
                    })
            })
            .or_else(|| {
                jwt_payload(&self.access_token).and_then(|payload| {
                    payload
                        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                        .or_else(|| payload.get("chatgpt_account_id"))
                        .and_then(Value::as_str)
                        .and_then(nonempty_lowercase)
                })
            })
    }

    pub fn reload(&self) -> Result<Self, CodexError> {
        match &self.source {
            AuthSource::File(path) => load_from_path(path),
            AuthSource::Hermes(path, hermes_id) => load_hermes_from_path(path, hermes_id),
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

    pub(super) fn update_and_save_if_current(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        id_token: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), CodexError> {
        let current = self.reload().map_err(|_| CodexError::AccountChanged)?;
        if current.document != self.document {
            return Err(CodexError::AccountChanged);
        }
        self.update_and_save(access_token, refresh_token, id_token, now)
    }

    fn update_and_save(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        id_token: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), CodexError> {
        let refreshed_at = now.to_rfc3339();

        match &self.source {
            AuthSource::File(path) => {
                set_string(&mut self.document, "/tokens/access_token", &access_token)?;
                if let Some(value) = refresh_token.as_deref() {
                    set_string(&mut self.document, "/tokens/refresh_token", value)?;
                    self.refresh_token = Some(value.to_owned());
                }
                if let Some(value) = id_token.as_deref() {
                    set_string(&mut self.document, "/tokens/id_token", value)?;
                }
                set_string(&mut self.document, "/last_refresh", &refreshed_at)?;
                self.access_token = access_token;
                self.last_refresh = Some(refreshed_at);
                save_file_document(path, &self.document)
            }
            AuthSource::Hermes(path, hermes_id) => {
                update_hermes_credential(
                    &mut self.document,
                    hermes_id,
                    &access_token,
                    refresh_token.as_deref(),
                    id_token.as_deref(),
                    &refreshed_at,
                )?;
                self.access_token = access_token;
                if let Some(value) = refresh_token {
                    self.refresh_token = Some(value);
                }
                self.last_refresh = Some(refreshed_at);
                save_file_document(path, &self.document)
            }
            #[cfg(target_os = "macos")]
            AuthSource::Keychain => {
                set_string(&mut self.document, "/tokens/access_token", &access_token)?;
                if let Some(value) = refresh_token.as_deref() {
                    set_string(&mut self.document, "/tokens/refresh_token", value)?;
                    self.refresh_token = Some(value.to_owned());
                }
                if let Some(value) = id_token.as_deref() {
                    set_string(&mut self.document, "/tokens/id_token", value)?;
                }
                set_string(&mut self.document, "/last_refresh", &refreshed_at)?;
                self.access_token = access_token;
                self.last_refresh = Some(refreshed_at);
                save_keychain_document(&self.document)
            }
        }
    }
}

pub(super) fn load_from_path(path: &Path) -> Result<CodexAuthState, CodexError> {
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
        crate::provider_environment::value("CODEX_HOME")
            .map(PathBuf::from)
            .as_deref(),
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
    let value = jwt_payload(token)?;
    DateTime::from_timestamp(value.get("exp")?.as_i64()?, 0)
}

fn jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn nonempty_lowercase(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn string_at(document: &Value, pointer: &str) -> Option<String> {
    document
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn auth_document_has_credentials(document: &Value) -> bool {
    document
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
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
    fn local_detection_only_accepts_a_usable_access_token() {
        assert!(auth_document_has_credentials(
            &json!({"tokens":{"access_token":"placeholder"}})
        ));
        assert!(!auth_document_has_credentials(
            &json!({"OPENAI_API_KEY":"placeholder"})
        ));
        assert!(!auth_document_has_credentials(
            &json!({"tokens":{"refresh_token":"placeholder"}})
        ));
        assert!(!auth_document_has_credentials(
            &json!({"tokens":{"access_token":""}})
        ));
    }

    #[test]
    fn account_identity_prefers_the_explicit_id_and_falls_back_to_the_id_token() {
        let state = CodexAuthState {
            source: AuthSource::File("auth.json".into()),
            document: json!({}),
            access_token: "access".into(),
            refresh_token: None,
            account_id: Some("  ACCOUNT-A  ".into()),
            last_refresh: None,
        };
        assert_eq!(state.account_identity().as_deref(), Some("account-a"));

        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "https://api.openai.com/auth": {"chatgpt_account_id": "Account-B"}
            }))
            .unwrap(),
        );
        let state = CodexAuthState {
            source: AuthSource::File("auth.json".into()),
            document: json!({"tokens": {"id_token": format!("header.{payload}.signature")}}),
            access_token: "access".into(),
            refresh_token: None,
            account_id: None,
            last_refresh: None,
        };
        assert_eq!(state.account_identity().as_deref(), Some("account-b"));
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

    #[test]
    fn refreshed_tokens_do_not_overwrite_credentials_changed_during_refresh() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        let original = json!({
            "tokens": {
                "access_token": "account-a-access",
                "refresh_token": "account-a-refresh",
                "account_id": "account-a"
            }
        });
        let replacement = json!({
            "tokens": {
                "access_token": "account-b-access",
                "refresh_token": "account-b-refresh",
                "account_id": "account-b"
            }
        });
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let mut state = super::load_from_path(&path).unwrap();
        fs::write(&path, serde_json::to_vec(&replacement).unwrap()).unwrap();

        let error = state
            .update_and_save_if_current(
                "rotated-account-a-access".into(),
                Some("rotated-account-a-refresh".into()),
                None,
                Utc::now(),
            )
            .unwrap_err();

        assert!(matches!(error, CodexError::AccountChanged));
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(persisted, replacement);
    }
}

pub(super) fn discover_identities_from_path(
    path: &Path,
) -> Vec<(String, super::accounts::CodexAuthSource)> {
    let mut identities = Vec::new();
    let Ok(text) = fs::read_to_string(path) else {
        return identities;
    };
    let Ok(document) = serde_json::from_str::<Value>(&text) else {
        return identities;
    };

    if document.get("version").is_some() && document.get("credential_pool").is_some() {
        if let Some(pool) = document
            .pointer("/credential_pool/openai-codex")
            .and_then(Value::as_array)
        {
            for cred in pool {
                if let Some(hermes_id) = cred.get("id").and_then(Value::as_str) {
                    if let Some(access_token) = cred
                        .get("access_token")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        let state = CodexAuthState {
                            source: AuthSource::Hermes(path.to_path_buf(), hermes_id.to_owned()),
                            document: document.clone(),
                            access_token: access_token.to_owned(),
                            refresh_token: cred
                                .get("refresh_token")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            account_id: None,
                            last_refresh: cred
                                .get("last_refresh")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        };
                        if let Some(identity) = state.account_identity() {
                            identities.push((
                                identity,
                                super::accounts::CodexAuthSource::Hermes(
                                    path.to_path_buf(),
                                    hermes_id.to_owned(),
                                ),
                            ));
                        }
                    }
                }
            }
        }
    } else {
        if let Ok(state) = load_from_path(path) {
            if let Some(identity) = state.account_identity() {
                identities.push((
                    identity,
                    super::accounts::CodexAuthSource::Home(path.to_path_buf()),
                ));
            }
        }
    }
    identities
}

pub(super) fn load_hermes_from_path(
    path: &Path,
    hermes_id: &str,
) -> Result<CodexAuthState, CodexError> {
    let text = fs::read_to_string(path).map_err(|_| CodexError::InvalidAuth)?;
    let document: Value = serde_json::from_str(&text).map_err(|_| CodexError::InvalidAuth)?;

    let pool = document
        .pointer("/credential_pool/openai-codex")
        .and_then(Value::as_array)
        .ok_or(CodexError::InvalidAuth)?;
    let cred = pool
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(hermes_id))
        .ok_or(CodexError::NotLoggedIn)?;

    let access_token = cred
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(CodexError::NotLoggedIn)?
        .to_owned();

    Ok(CodexAuthState {
        source: AuthSource::Hermes(path.to_path_buf(), hermes_id.to_owned()),
        refresh_token: cred
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        account_id: None,
        last_refresh: cred
            .get("last_refresh")
            .and_then(Value::as_str)
            .map(str::to_owned),
        document,
        access_token,
    })
}

fn update_hermes_credential(
    document: &mut Value,
    hermes_id: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    _id_token: Option<&str>,
    refreshed_at: &str,
) -> Result<(), CodexError> {
    let pool = document
        .pointer_mut("/credential_pool/openai-codex")
        .and_then(Value::as_array_mut)
        .ok_or(CodexError::AuthWrite)?;
    let cred = pool
        .iter_mut()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(hermes_id))
        .ok_or(CodexError::AuthWrite)?;

    if let Some(obj) = cred.as_object_mut() {
        obj.insert(
            "access_token".to_string(),
            Value::String(access_token.to_string()),
        );
        if let Some(rt) = refresh_token {
            obj.insert("refresh_token".to_string(), Value::String(rt.to_string()));
        }
        obj.insert(
            "last_refresh".to_string(),
            Value::String(refreshed_at.to_string()),
        );
        Ok(())
    } else {
        Err(CodexError::AuthWrite)
    }
}
