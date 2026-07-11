use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::providers::credential_store::{decode_go_keyring_value, read_generic_password};

#[derive(Debug, Clone)]
pub struct AntigravityToken {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expiry: Option<DateTime<Utc>>,
}

pub fn load_token() -> Option<AntigravityToken> {
    let raw = read_generic_password("gemini", "antigravity").ok()??;
    extract_token(&raw)
}

pub fn has_local_credentials() -> bool {
    load_token().is_some()
}

pub fn extract_token(raw: &[u8]) -> Option<AntigravityToken> {
    let unwrapped =
        decode_go_keyring_value(raw).or_else(|| String::from_utf8(raw.to_vec()).ok())?;
    let text = unwrapped.trim();
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if let Some(token) = token_from_value(&value) {
            return Some(token);
        }
        if let Some(token) = value.as_str().and_then(non_empty) {
            return Some(AntigravityToken {
                access_token: Some(token.into()),
                refresh_token: None,
                expiry: None,
            });
        }
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

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine};

    use super::extract_token;

    #[test]
    fn extracts_nested_go_keyring_token() {
        let json = r#"{"token":{"access_token":"access","refresh_token":"refresh","expiry":"2026-07-12T10:00:00Z"}}"#;
        let wrapped = format!("go-keyring-base64:{}", STANDARD.encode(json));
        let token = extract_token(wrapped.as_bytes()).unwrap();
        assert_eq!(token.access_token.as_deref(), Some("access"));
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
        assert!(token.expiry.is_some());
    }
}
