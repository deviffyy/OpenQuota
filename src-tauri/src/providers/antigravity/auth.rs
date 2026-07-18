use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::providers::credential_store::{decode_go_keyring_value, read_generic_password};

use super::AntigravityError;

const ACCESS_TOKEN_CACHE_VERSION: u8 = 1;
const ACCESS_TOKEN_CACHE_LIMIT: u64 = 64 * 1024;
const ACCESS_TOKEN_EXPIRY_BUFFER_MILLIS: i64 = 60_000;
const DEFAULT_ACCESS_TOKEN_LIFETIME_SECONDS: f64 = 3_600.0;

#[derive(Debug, Clone)]
pub struct AntigravityToken {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expiry: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct AccessTokenCache {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedAccessToken {
    version: u8,
    access_token: String,
    expires_at_millis: i64,
    credential_fingerprint: [u8; 32],
}

impl AccessTokenCache {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self, refresh_token: Option<&str>, now: DateTime<Utc>) -> Option<String> {
        let Some(expected_fingerprint) = credential_fingerprint(refresh_token) else {
            self.discard();
            return None;
        };
        let bytes = match read_bounded(&self.path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(_) => {
                crate::app_warn!("auth:antigravity", "cached access token could not be read");
                return None;
            }
        };
        let cached = serde_json::from_slice::<CachedAccessToken>(&bytes).ok();
        let usable = cached.as_ref().is_some_and(|cached| {
            cached.version == ACCESS_TOKEN_CACHE_VERSION
                && !cached.access_token.trim().is_empty()
                && cached.credential_fingerprint == expected_fingerprint
                && cached.expires_at_millis
                    > now
                        .timestamp_millis()
                        .saturating_add(ACCESS_TOKEN_EXPIRY_BUFFER_MILLIS)
        });
        if usable {
            cached.map(|cached| cached.access_token)
        } else {
            self.discard();
            None
        }
    }

    pub fn store(
        &self,
        access_token: &str,
        expires_in_seconds: f64,
        refresh_token: Option<&str>,
        now: DateTime<Utc>,
    ) {
        let access_token = access_token.trim();
        let Some(credential_fingerprint) = credential_fingerprint(refresh_token) else {
            return;
        };
        if access_token.is_empty() {
            return;
        }
        let lifetime = if expires_in_seconds.is_finite() && expires_in_seconds > 0.0 {
            expires_in_seconds
        } else {
            DEFAULT_ACCESS_TOKEN_LIFETIME_SECONDS
        };
        let lifetime_millis = (lifetime * 1_000.0).min(i64::MAX as f64) as i64;
        let cached = CachedAccessToken {
            version: ACCESS_TOKEN_CACHE_VERSION,
            access_token: access_token.to_owned(),
            expires_at_millis: now.timestamp_millis().saturating_add(lifetime_millis),
            credential_fingerprint,
        };
        let result = serde_json::to_vec(&cached)
            .map_err(io::Error::other)
            .and_then(|bytes| write_private_atomic(&self.path, &bytes));
        if result.is_err() {
            crate::app_warn!("auth:antigravity", "cached access token could not be saved");
        }
    }

    pub fn discard(&self) {
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != io::ErrorKind::NotFound {
                crate::app_warn!(
                    "auth:antigravity",
                    "cached access token could not be discarded"
                );
            }
        }
    }
}

pub fn load_token() -> Result<Option<AntigravityToken>, AntigravityError> {
    let Some(raw) = read_generic_password("gemini", "antigravity")
        .map_err(|_| AntigravityError::CredentialStoreUnreadable)?
    else {
        return Ok(None);
    };
    extract_token(&raw)
        .map(Some)
        .ok_or(AntigravityError::InvalidCredentialData)
}

pub fn has_local_credentials() -> bool {
    credential_state_is_actionable(load_token())
}

fn credential_state_is_actionable(
    state: Result<Option<AntigravityToken>, AntigravityError>,
) -> bool {
    !matches!(state, Ok(None))
}

pub fn credential_fingerprint(refresh_token: Option<&str>) -> Option<[u8; 32]> {
    let refresh_token = refresh_token
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(Sha256::digest(refresh_token.as_bytes()).into())
}

pub fn extract_token(raw: &[u8]) -> Option<AntigravityToken> {
    let encoded = std::str::from_utf8(raw)
        .ok()?
        .trim()
        .trim_start_matches('\u{feff}')
        .trim();
    let unwrapped = if encoded.starts_with("go-keyring-base64:") {
        decode_go_keyring_value(encoded.as_bytes())?
    } else {
        encoded.to_owned()
    };
    let text = unwrapped.trim().trim_start_matches('\u{feff}').trim();
    match serde_json::from_str::<Value>(text) {
        Ok(value) => {
            if let Some(token) = token_from_value(&value) {
                return Some(token);
            }
            return value
                .as_str()
                .and_then(non_empty)
                .map(|token| AntigravityToken {
                    access_token: Some(token.into()),
                    refresh_token: None,
                    expiry: None,
                });
        }
        Err(_) if text.starts_with('{') || text.starts_with('[') => return None,
        Err(_) => {}
    }
    let token = text.strip_prefix("Bearer ").unwrap_or(text).trim();
    non_empty(token).map(|token| AntigravityToken {
        access_token: Some(token.into()),
        refresh_token: None,
        expiry: None,
    })
}

fn token_from_value(value: &Value) -> Option<AntigravityToken> {
    let object = value.as_object()?;
    let source = object
        .get("token")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let access_token = first_string(
        source,
        &[
            "access_token",
            "accessToken",
            "token",
            "id_token",
            "idToken",
            "bearerToken",
            "auth_token",
            "authToken",
        ],
    );
    let refresh_token = first_string(source, &["refresh_token", "refreshToken"]);
    if access_token.is_none() && refresh_token.is_none() {
        for key in ["tokens", "oauth", "oauth2", "credentials", "auth"] {
            if let Some(token) = object.get(key).and_then(token_from_value) {
                return Some(token);
            }
        }
        return None;
    }
    let expiry = first_string(source, &["expiry", "expires_at", "expiresAt"])
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.to_utc());
    Some(AntigravityToken {
        access_token,
        refresh_token,
        expiry,
    })
}

fn first_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str).and_then(non_empty))
        .map(str::to_owned)
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn read_bounded(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(ACCESS_TOKEN_CACHE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > ACCESS_TOKEN_CACHE_LIMIT {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(bytes))
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let mut temporary = NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::{engine::general_purpose::STANDARD, Engine};
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use super::{
        credential_fingerprint, credential_state_is_actionable, extract_token, AccessTokenCache,
        AntigravityToken,
    };
    use crate::providers::antigravity::AntigravityError;

    #[test]
    fn extracts_nested_go_keyring_token() {
        let json = r#"{"token":{"access_token":"access","refresh_token":"refresh","expiry":"2026-07-12T10:00:00Z"}}"#;
        let wrapped = format!("go-keyring-base64:{}", STANDARD.encode(json));
        let token = extract_token(wrapped.as_bytes()).unwrap();
        assert_eq!(token.access_token.as_deref(), Some("access"));
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
        assert!(token.expiry.is_some());
    }

    #[test]
    fn refresh_credential_fingerprint_is_stable_and_secret_free() {
        let first = credential_fingerprint(Some("refresh-a")).unwrap();
        assert_eq!(first, credential_fingerprint(Some(" refresh-a ")).unwrap());
        assert_ne!(first, credential_fingerprint(Some("refresh-b")).unwrap());
        assert!(credential_fingerprint(None).is_none());
        assert!(credential_fingerprint(Some("   ")).is_none());
    }

    #[test]
    fn credential_store_failures_remain_actionable_during_detection() {
        assert!(!credential_state_is_actionable(Ok(None)));
        assert!(credential_state_is_actionable(Ok(Some(AntigravityToken {
            access_token: Some("access".into()),
            refresh_token: None,
            expiry: None,
        }))));
        assert!(credential_state_is_actionable(Err(
            AntigravityError::CredentialStoreUnreadable
        )));
    }

    #[test]
    fn structured_material_without_tokens_is_never_used_as_a_bearer_token() {
        for value in [
            br#"{"account":{}}"#.as_slice(),
            br#"[]"#.as_slice(),
            br#"{"broken""#.as_slice(),
            br#"[broken"#.as_slice(),
        ] {
            assert!(extract_token(value).is_none());
        }
    }

    #[test]
    fn malformed_go_keyring_wrappers_are_never_used_as_bearer_tokens() {
        for value in [
            b"go-keyring-base64:not-base64!".as_slice(),
            b"go-keyring-base64:/w==".as_slice(),
        ] {
            assert!(extract_token(value).is_none());
        }
    }

    #[test]
    fn json_strings_and_unstructured_tokens_keep_the_supported_fallbacks() {
        assert_eq!(
            extract_token(br#""quoted-token""#)
                .unwrap()
                .access_token
                .as_deref(),
            Some("quoted-token")
        );
        assert_eq!(
            extract_token(b"Bearer bearer-token")
                .unwrap()
                .access_token
                .as_deref(),
            Some("bearer-token")
        );
        assert_eq!(
            extract_token(b"raw-token").unwrap().access_token.as_deref(),
            Some("raw-token")
        );
    }

    #[test]
    fn cached_access_tokens_are_persistent_and_bound_to_the_refresh_credential() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        let cache = AccessTokenCache::new(path.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();

        cache.store("first", 3_600.0, Some("refresh-a"), now);
        assert_eq!(cache.load(Some("refresh-a"), now).as_deref(), Some("first"));

        cache.store("second", 3_600.0, Some("refresh-a"), now);
        assert_eq!(
            cache.load(Some("refresh-a"), now).as_deref(),
            Some("second")
        );
        assert!(cache.load(Some("refresh-b"), now).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn cached_access_tokens_expire_early_and_malformed_files_are_discarded() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        let cache = AccessTokenCache::new(path.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();

        cache.store("short-lived", 30.0, Some("refresh"), now);
        assert!(cache.load(Some("refresh"), now).is_none());
        assert!(!path.exists());

        fs::write(&path, b"{not-json").unwrap();
        assert!(cache.load(Some("refresh"), now).is_none());
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cached_access_token_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let path = directory.path().join("nested").join("auth.json");
        let cache = AccessTokenCache::new(path.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();

        cache.store("secret", 3_600.0, Some("refresh"), now);

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
