use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::providers::credential_store::{read_generic_password, write_generic_password};

use super::ClaudeError;

const DEFAULT_API_BASE: &str = "https://api.anthropic.com";
const DEFAULT_REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const DEFAULT_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const NON_PROD_CLIENT_ID: &str = "22422756-60c9-4084-8eb7-27705fd5cf9a";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeOAuth {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<f64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ClaudeCredentialsFile {
    claude_ai_oauth: Option<ClaudeOAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialSource {
    File(PathBuf),
    Keychain { service: String, account: String },
    Environment,
}

#[derive(Debug, Clone)]
pub struct ClaudeCredential {
    pub oauth: ClaudeOAuth,
    source: CredentialSource,
    document: ClaudeCredentialsFile,
    pub inference_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCredentialGeneration {
    entries: Vec<ClaudeCredentialGenerationEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeCredentialGenerationEntry {
    source: CredentialSource,
    oauth_fingerprint: [u8; 32],
}

impl ClaudeCredentialGeneration {
    pub fn from_candidates(candidates: &[ClaudeCredential]) -> Self {
        Self {
            entries: candidates
                .iter()
                .filter(|candidate| !candidate.inference_only)
                .map(|candidate| ClaudeCredentialGenerationEntry {
                    source: candidate.source.clone(),
                    oauth_fingerprint: oauth_generation_fingerprint(&candidate.oauth),
                })
                .collect(),
        }
    }

    pub fn replacing(&self, credential: &ClaudeCredential) -> Option<Self> {
        let mut updated = self.clone();
        let entry = updated
            .entries
            .iter_mut()
            .find(|entry| entry.source == credential.source)?;
        entry.oauth_fingerprint = oauth_generation_fingerprint(&credential.oauth);
        Some(updated)
    }
}

impl ClaudeCredential {
    pub fn access_token(&self) -> Option<&str> {
        self.oauth
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn has_profile_scope(&self) -> bool {
        if self.inference_only {
            return false;
        }
        self.oauth.scopes.as_ref().is_none_or(|scopes| {
            scopes.is_empty() || scopes.iter().any(|scope| scope == "user:profile")
        })
    }

    pub fn needs_refresh(&self, now_millis: i64) -> bool {
        self.oauth
            .expires_at
            .is_some_and(|expires| expires - now_millis as f64 <= 5.0 * 60.0 * 1000.0)
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        let access = Sha256::digest(
            self.oauth
                .access_token
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        let refresh = Sha256::digest(
            self.oauth
                .refresh_token
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        let mut pair = Sha256::new();
        pair.update(access);
        pair.update(refresh);
        pair.finalize().into()
    }

    pub fn update_and_save(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<f64>,
        now_millis: i64,
        expected_generation: &ClaudeCredentialGeneration,
    ) -> Result<bool, ClaudeError> {
        self.update_and_save_with_generation(
            access_token,
            refresh_token,
            expires_in,
            now_millis,
            expected_generation,
            credential_generation,
        )
    }

    fn update_and_save_with_generation<F>(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<f64>,
        now_millis: i64,
        expected_generation: &ClaudeCredentialGeneration,
        current_generation: F,
    ) -> Result<bool, ClaudeError>
    where
        F: FnOnce() -> ClaudeCredentialGeneration,
    {
        if current_generation() != *expected_generation {
            return Err(ClaudeError::CredentialsChanged);
        }
        self.oauth.access_token = Some(access_token);
        if let Some(refresh_token) = refresh_token.filter(|value| !value.is_empty()) {
            self.oauth.refresh_token = Some(refresh_token);
        }
        if let Some(expires_in) = expires_in {
            self.oauth.expires_at = Some(now_millis as f64 + expires_in * 1000.0);
        }
        self.document.claude_ai_oauth = Some(self.oauth.clone());
        let bytes = serde_json::to_vec(&self.document).map_err(|_| ClaudeError::AuthWrite)?;
        match &self.source {
            CredentialSource::File(path) => {
                write_private_file_atomic(path, &bytes)?;
                Ok(true)
            }
            CredentialSource::Keychain { service, account } => {
                write_generic_password(service, account, &bytes)
                    .map_err(|_| ClaudeError::AuthWrite)?;
                Ok(true)
            }
            CredentialSource::Environment => Ok(false),
        }
    }
}

pub fn credential_generation() -> ClaudeCredentialGeneration {
    ClaudeCredentialGeneration::from_candidates(&load_candidates())
}

fn oauth_generation_fingerprint(oauth: &ClaudeOAuth) -> [u8; 32] {
    let mut fingerprint = Sha256::new();
    update_optional_text(&mut fingerprint, oauth.access_token.as_deref());
    update_optional_text(&mut fingerprint, oauth.refresh_token.as_deref());
    match oauth.expires_at {
        Some(value) => {
            fingerprint.update([1]);
            fingerprint.update(value.to_bits().to_le_bytes());
        }
        None => fingerprint.update([0]),
    }
    update_optional_text(&mut fingerprint, oauth.subscription_type.as_deref());
    update_optional_text(&mut fingerprint, oauth.rate_limit_tier.as_deref());
    match oauth.scopes.as_ref() {
        Some(scopes) => {
            fingerprint.update([1]);
            fingerprint.update((scopes.len() as u64).to_le_bytes());
            for scope in scopes {
                update_text(&mut fingerprint, scope);
            }
        }
        None => fingerprint.update([0]),
    }
    fingerprint.finalize().into()
}

fn update_optional_text(fingerprint: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            fingerprint.update([1]);
            update_text(fingerprint, value);
        }
        None => fingerprint.update([0]),
    }
}

fn update_text(fingerprint: &mut Sha256, value: &str) {
    fingerprint.update((value.len() as u64).to_le_bytes());
    fingerprint.update(value.as_bytes());
}

fn write_private_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), ClaudeError> {
    let parent = path.parent().ok_or(ClaudeError::AuthWrite)?;
    fs::create_dir_all(parent).map_err(|_| ClaudeError::AuthWrite)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|_| ClaudeError::AuthWrite)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| ClaudeError::AuthWrite)?;
    }
    temporary
        .write_all(bytes)
        .map_err(|_| ClaudeError::AuthWrite)?;
    temporary.flush().map_err(|_| ClaudeError::AuthWrite)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| ClaudeError::AuthWrite)?;
    temporary
        .persist(path)
        .map_err(|_| ClaudeError::AuthWrite)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ClaudeOAuthConfig {
    pub usage_url: String,
    pub refresh_url: String,
    pub client_id: String,
}

pub fn has_local_credentials() -> bool {
    !load_candidates().is_empty()
}

pub fn has_desktop_app_data() -> bool {
    #[cfg(target_os = "macos")]
    {
        has_desktop_app_material_at(&home_directory())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(any(target_os = "macos", test))]
fn has_desktop_app_material_at(home: &Path) -> bool {
    let root = home
        .join("Library")
        .join("Application Support")
        .join("Claude");
    let Ok(document) = fs::read(root.join("config.json")) else {
        return false;
    };
    let Ok(config) = serde_json::from_slice::<serde_json::Value>(&document) else {
        return false;
    };
    let has_token_cache = ["oauth:tokenCacheV2", "oauth:tokenCache"]
        .into_iter()
        .any(|key| {
            config
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        });
    has_token_cache
        && [root.join("Cookies"), root.join("Network").join("Cookies")]
            .into_iter()
            .any(|path| path.is_file())
}

pub fn load_candidates() -> Vec<ClaudeCredential> {
    let mut stored = Vec::new();
    for (service, account) in keychain_candidates() {
        let Ok(Some(bytes)) = read_generic_password(&service, &account) else {
            continue;
        };
        if let Some(credential) = parse_candidate(
            &bytes,
            CredentialSource::Keychain { service, account },
            false,
        ) {
            stored.push(credential);
            break;
        }
    }
    let path = credential_path();
    if let Ok(bytes) = fs::read(&path) {
        if let Some(credential) = parse_candidate(&bytes, CredentialSource::File(path), false) {
            stored.push(credential);
        }
    }

    let Some(environment_token) = env_text("CLAUDE_CODE_OAUTH_TOKEN") else {
        return stored;
    };
    let base = stored.first().cloned().unwrap_or(ClaudeCredential {
        oauth: ClaudeOAuth::default(),
        source: CredentialSource::Environment,
        document: ClaudeCredentialsFile::default(),
        inference_only: true,
    });
    let mut oauth = base.oauth;
    oauth.access_token = Some(environment_token);
    let environment = ClaudeCredential {
        oauth,
        source: CredentialSource::Environment,
        document: base.document,
        inference_only: true,
    };
    let live = stored
        .into_iter()
        .filter(|candidate| candidate.has_profile_scope())
        .collect::<Vec<_>>();
    if live.is_empty() {
        vec![environment]
    } else {
        live.into_iter().chain(Some(environment)).collect()
    }
}

pub fn oauth_config() -> Result<ClaudeOAuthConfig, ClaudeError> {
    let (base, refresh_url, default_client_id, _) = resolved_oauth_settings();
    let usage_url = format!("{base}/api/oauth/usage");
    validate_http_url(&usage_url)?;
    validate_http_url(&refresh_url)?;
    Ok(ClaudeOAuthConfig {
        usage_url,
        refresh_url,
        client_id: env_text("CLAUDE_CODE_OAUTH_CLIENT_ID").unwrap_or(default_client_id),
    })
}

fn resolved_oauth_settings() -> (String, String, String, &'static str) {
    let mut base = DEFAULT_API_BASE.to_owned();
    let mut refresh = DEFAULT_REFRESH_URL.to_owned();
    let mut client_id = DEFAULT_CLIENT_ID.to_owned();
    let mut suffix = "";
    if env_text("USER_TYPE").as_deref() == Some("ant") && env_flag("USE_LOCAL_OAUTH") {
        base = env_text("CLAUDE_LOCAL_OAUTH_API_BASE")
            .unwrap_or_else(|| "http://localhost:8000".into())
            .trim_end_matches('/')
            .to_owned();
        refresh = format!("{base}/v1/oauth/token");
        client_id = NON_PROD_CLIENT_ID.into();
        suffix = "-local-oauth";
    } else if env_text("USER_TYPE").as_deref() == Some("ant") && env_flag("USE_STAGING_OAUTH") {
        base = "https://api-staging.anthropic.com".into();
        refresh = "https://platform.staging.ant.dev/v1/oauth/token".into();
        client_id = NON_PROD_CLIENT_ID.into();
        suffix = "-staging-oauth";
    }
    if let Some(custom) = env_text("CLAUDE_CODE_CUSTOM_OAUTH_URL") {
        base = custom.trim_end_matches('/').to_owned();
        refresh = format!("{base}/v1/oauth/token");
        suffix = "-custom-oauth";
    }
    (base, refresh, client_id, suffix)
}

fn validate_http_url(value: &str) -> Result<(), ClaudeError> {
    let url = reqwest::Url::parse(value).map_err(|_| ClaudeError::InvalidOAuthUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(ClaudeError::InvalidOAuthUrl);
    }
    Ok(())
}

fn parse_candidate(
    bytes: &[u8],
    source: CredentialSource,
    inference_only: bool,
) -> Option<ClaudeCredential> {
    let document = parse_credentials(bytes)?;
    let oauth = document.claude_ai_oauth.clone()?;
    oauth
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(ClaudeCredential {
        oauth,
        source,
        document,
        inference_only,
    })
}

fn parse_credentials(bytes: &[u8]) -> Option<ClaudeCredentialsFile> {
    serde_json::from_slice(bytes).ok().or_else(|| {
        let text = std::str::from_utf8(bytes).ok()?.trim();
        if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let decoded = (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        serde_json::from_slice(&decoded).ok()
    })
}

fn credential_path() -> PathBuf {
    claude_home().join(".credentials.json")
}

pub fn claude_home() -> PathBuf {
    env_text("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_directory().join(".claude"))
}

fn keychain_candidates() -> Vec<(String, String)> {
    let suffix = resolved_oauth_settings().3;
    let service = format!("Claude Code{suffix}-credentials");
    let base = if let Some(config_dir) = env_text("CLAUDE_CONFIG_DIR") {
        let normalized = config_dir.replace('\\', "/");
        let hash = format!("{:x}", Sha256::digest(normalized.as_bytes()));
        vec![format!("{service}-{}", &hash[..8]), service]
    } else {
        vec![service]
    };
    let user = env_text("USER")
        .or_else(|| env_text("USERNAME"))
        .unwrap_or_default();
    base.into_iter()
        .flat_map(|service| {
            let current = (service.clone(), user.clone());
            [current, (service, String::new())]
        })
        .collect()
}

fn env_text(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> bool {
    env_text(name)
        .map(|value| {
            !matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(false)
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

    use chrono::Utc;
    use tempfile::tempdir;

    use super::{
        has_desktop_app_material_at, parse_credentials, write_private_file_atomic,
        ClaudeCredential, ClaudeCredentialGeneration, ClaudeCredentialsFile, ClaudeOAuth,
        CredentialSource,
    };
    use crate::providers::claude::ClaudeError;

    #[test]
    fn parses_claude_credentials_and_hex_fallback() {
        let raw = br#"{"claudeAiOauth":{"accessToken":"placeholder","scopes":["user:profile"]}}"#;
        let parsed = parse_credentials(raw).unwrap();
        assert_eq!(
            parsed.claude_ai_oauth.unwrap().access_token.as_deref(),
            Some("placeholder")
        );
        let hex = raw
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let _: ClaudeCredentialsFile = parse_credentials(hex.as_bytes()).unwrap();
    }

    #[test]
    fn credential_write_failures_are_typed_and_do_not_expose_tokens() {
        let directory = tempdir().unwrap();
        let blocked_parent = directory.path().join("not-a-directory");
        fs::write(&blocked_parent, b"block directory creation").unwrap();
        let mut credential = ClaudeCredential {
            oauth: ClaudeOAuth::default(),
            source: CredentialSource::File(blocked_parent.join("credentials.json")),
            document: ClaudeCredentialsFile::default(),
            inference_only: false,
        };
        let generation = ClaudeCredentialGeneration::from_candidates(&[credential.clone()]);

        let error = credential
            .update_and_save_with_generation(
                "secret-access".into(),
                Some("secret-refresh".into()),
                Some(3600.0),
                Utc::now().timestamp_millis(),
                &generation,
                || generation.clone(),
            )
            .unwrap_err();

        assert!(matches!(error, ClaudeError::AuthWrite));
        assert!(!error.to_string().contains("secret-access"));
        assert!(!error.to_string().contains("secret-refresh"));
    }

    #[test]
    fn atomic_credential_write_replaces_an_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        fs::write(&path, b"old credential").unwrap();

        write_private_file_atomic(&path, b"new credential").unwrap();

        assert_eq!(fs::read(path).unwrap(), b"new credential");
    }

    #[test]
    fn credential_rotation_refuses_to_overwrite_a_changed_login() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        fs::write(&path, b"original credential").unwrap();
        let mut credential = ClaudeCredential {
            oauth: ClaudeOAuth {
                access_token: Some("account-a".into()),
                refresh_token: Some("refresh-a".into()),
                ..ClaudeOAuth::default()
            },
            source: CredentialSource::File(path.clone()),
            document: ClaudeCredentialsFile::default(),
            inference_only: false,
        };
        let expected = ClaudeCredentialGeneration::from_candidates(&[credential.clone()]);
        let changed = ClaudeCredential {
            oauth: ClaudeOAuth {
                access_token: Some("account-b".into()),
                refresh_token: Some("refresh-b".into()),
                ..ClaudeOAuth::default()
            },
            ..credential.clone()
        };
        let current = ClaudeCredentialGeneration::from_candidates(&[changed]);

        let error = credential
            .update_and_save_with_generation(
                "rotated-a".into(),
                Some("rotated-refresh-a".into()),
                Some(3600.0),
                Utc::now().timestamp_millis(),
                &expected,
                || current,
            )
            .unwrap_err();

        assert!(matches!(error, ClaudeError::CredentialsChanged));
        assert_eq!(credential.oauth.access_token.as_deref(), Some("account-a"));
        assert_eq!(fs::read(path).unwrap(), b"original credential");
    }

    #[test]
    fn credential_generation_tracks_order_source_and_complete_oauth_state() {
        let credential = |source: CredentialSource, access: &str, refresh: &str| ClaudeCredential {
            oauth: ClaudeOAuth {
                access_token: Some(access.into()),
                refresh_token: Some(refresh.into()),
                expires_at: Some(1.0),
                subscription_type: Some("pro".into()),
                rate_limit_tier: Some("tier-a".into()),
                scopes: Some(vec!["user:profile".into()]),
            },
            source,
            document: ClaudeCredentialsFile::default(),
            inference_only: false,
        };
        let file = credential(
            CredentialSource::File("credentials.json".into()),
            "access-a",
            "refresh-a",
        );
        let keychain = credential(
            CredentialSource::Keychain {
                service: "service".into(),
                account: "account".into(),
            },
            "access-b",
            "refresh-b",
        );
        let original =
            ClaudeCredentialGeneration::from_candidates(&[file.clone(), keychain.clone()]);

        assert_ne!(
            original,
            ClaudeCredentialGeneration::from_candidates(&[keychain.clone(), file.clone()])
        );
        let mut metadata_changed = file.clone();
        metadata_changed.oauth.rate_limit_tier = Some("tier-b".into());
        assert_ne!(
            original,
            ClaudeCredentialGeneration::from_candidates(&[metadata_changed, keychain.clone()])
        );
        let mut rotated = file;
        rotated.oauth.access_token = Some("rotated".into());
        let replaced = original.replacing(&rotated).unwrap();
        assert_eq!(
            replaced,
            ClaudeCredentialGeneration::from_candidates(&[rotated, keychain])
        );
    }

    #[test]
    fn credential_fingerprint_covers_the_complete_token_pair() {
        let credential = |access: &str, refresh: &str| ClaudeCredential {
            oauth: ClaudeOAuth {
                access_token: Some(access.into()),
                refresh_token: Some(refresh.into()),
                ..ClaudeOAuth::default()
            },
            source: CredentialSource::Environment,
            document: ClaudeCredentialsFile::default(),
            inference_only: false,
        };

        let original = credential("shared-access", "refresh-a").fingerprint();
        assert_eq!(
            original,
            credential("shared-access", "refresh-a").fingerprint()
        );
        assert_ne!(
            original,
            credential("shared-access", "refresh-b").fingerprint()
        );
        assert_ne!(
            original,
            credential("different-access", "refresh-a").fingerprint()
        );
    }

    #[test]
    fn desktop_detection_requires_token_cache_and_cookie_database() {
        let directory = tempdir().unwrap();
        let root = directory
            .path()
            .join("Library")
            .join("Application Support")
            .join("Claude");
        fs::create_dir_all(root.join("Network")).unwrap();

        assert!(!has_desktop_app_material_at(directory.path()));
        fs::write(
            root.join("config.json"),
            br#"{"oauth:tokenCacheV2":"encrypted-cache"}"#,
        )
        .unwrap();
        assert!(!has_desktop_app_material_at(directory.path()));

        fs::write(root.join("Network").join("Cookies"), b"sqlite").unwrap();
        assert!(has_desktop_app_material_at(directory.path()));
    }

    #[test]
    fn desktop_detection_rejects_empty_or_invalid_cache_documents() {
        let directory = tempdir().unwrap();
        let root = directory
            .path()
            .join("Library")
            .join("Application Support")
            .join("Claude");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cookies"), b"sqlite").unwrap();

        fs::write(root.join("config.json"), br#"{"oauth:tokenCache":""}"#).unwrap();
        assert!(!has_desktop_app_material_at(directory.path()));
        fs::write(root.join("config.json"), b"not-json").unwrap();
        assert!(!has_desktop_app_material_at(directory.path()));
    }
}
