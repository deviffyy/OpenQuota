use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::providers::credential_store::{read_generic_password, write_generic_password};

use super::ClaudeError;

const DEFAULT_API_BASE: &str = "https://api.anthropic.com";
const DEFAULT_REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const DEFAULT_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const NON_PROD_CLIENT_ID: &str = "22422756-60c9-4084-8eb7-27705fd5cf9a";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
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

    pub fn update_and_save(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<f64>,
        now_millis: i64,
    ) -> Result<(), ClaudeError> {
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
                let parent = path.parent().ok_or(ClaudeError::AuthWrite)?;
                fs::create_dir_all(parent).map_err(|_| ClaudeError::AuthWrite)?;
                let temporary = path.with_extension("json.tmp-openquota");
                fs::write(&temporary, &bytes).map_err(|_| ClaudeError::AuthWrite)?;
                fs::rename(temporary, path).map_err(|_| ClaudeError::AuthWrite)
            }
            CredentialSource::Keychain { service, account } => {
                write_generic_password(service, account, &bytes).map_err(|_| ClaudeError::AuthWrite)
            }
            CredentialSource::Environment => Ok(()),
        }
    }
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
    let home = home_directory();
    let mut paths = vec![
        home.join("Library")
            .join("Application Support")
            .join("Claude Code"),
        home.join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude-code"),
        home.join(".config").join("Claude"),
        home.join(".config").join("Claude Code"),
    ];
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(app_data.join("Claude"));
        paths.push(app_data.join("Claude Code"));
    }
    paths.into_iter().any(|path| path.is_dir())
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
        parse_credentials, ClaudeCredential, ClaudeCredentialsFile, ClaudeOAuth, CredentialSource,
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

        let error = credential
            .update_and_save(
                "secret-access".into(),
                Some("secret-refresh".into()),
                Some(3600.0),
                Utc::now().timestamp_millis(),
            )
            .unwrap_err();

        assert!(matches!(error, ClaudeError::AuthWrite));
        assert!(!error.to_string().contains("secret-access"));
        assert!(!error.to_string().contains("secret-refresh"));
    }
}
