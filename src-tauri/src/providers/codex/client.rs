use std::{collections::HashMap, time::Duration};

use reqwest::{blocking::Client, header::HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use super::CodexError;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REFRESH_URL: &str = "https://auth.openai.com/oauth/token";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Clone)]
pub struct UsageResponse {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Deserialize)]
pub struct TokenRefresh {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
}

pub struct CodexClient {
    client: Client,
    refresh_url: String,
    usage_url: String,
}

impl CodexClient {
    pub fn new() -> Result<Self, CodexError> {
        Self::with_endpoints(USAGE_URL, REFRESH_URL, Duration::from_secs(15))
    }

    fn with_endpoints(
        usage_url: &str,
        refresh_url: &str,
        timeout: Duration,
    ) -> Result<Self, CodexError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| CodexError::ConnectionFailed)?;
        Ok(Self {
            client,
            refresh_url: refresh_url.to_owned(),
            usage_url: usage_url.to_owned(),
        })
    }

    pub fn fetch_usage(
        &self,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<UsageResponse, CodexError> {
        let mut request = self
            .client
            .get(&self.usage_url)
            .bearer_auth(access_token)
            .header("Accept", "application/json");
        if let Some(account_id) = account_id.filter(|value| !value.is_empty()) {
            request = request.header("ChatGPT-Account-Id", account_id);
        }
        let response = request.send().map_err(|_| CodexError::ConnectionFailed)?;
        let status = response.status();
        let headers = normalized_headers(response.headers());
        let text = response.text().map_err(|_| CodexError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() && body.is_null() {
            return Err(CodexError::InvalidResponse);
        }
        Ok(UsageResponse {
            status,
            headers,
            body,
        })
    }

    pub fn refresh_token(&self, refresh_token: &str) -> Result<TokenRefresh, CodexError> {
        let response = self
            .client
            .post(&self.refresh_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", refresh_token),
            ])
            .send()
            .map_err(|_| CodexError::ConnectionFailed)?;
        let status = response.status();
        let body: Value = response.json().map_err(|_| {
            if status.is_success() {
                CodexError::InvalidResponse
            } else {
                CodexError::RequestFailed(status.as_u16())
            }
        })?;

        if !status.is_success() {
            let code = oauth_error_code(&body);
            return Err(match code.as_deref() {
                Some("refresh_token_expired") => CodexError::SessionExpired,
                Some("refresh_token_reused") => CodexError::TokenConflict,
                Some("refresh_token_invalidated") => CodexError::TokenRevoked,
                _ => CodexError::RequestFailed(status.as_u16()),
            });
        }
        let refreshed: TokenRefresh =
            serde_json::from_value(body).map_err(|_| CodexError::InvalidResponse)?;
        if refreshed.access_token.is_empty() {
            return Err(CodexError::SessionExpired);
        }
        Ok(refreshed)
    }
}

fn normalized_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
        })
        .collect()
}

fn oauth_error_code(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("code").and_then(Value::as_str))
                .or_else(|| error.get("error").and_then(Value::as_str))
        })
        .or_else(|| body.get("code").and_then(Value::as_str))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use reqwest::StatusCode;

    use super::CodexClient;
    use crate::providers::{codex::CodexError, test_http};

    fn client(base: &str) -> CodexClient {
        CodexClient::with_endpoints(
            &format!("{base}/usage"),
            &format!("{base}/token"),
            Duration::from_secs(1),
        )
        .unwrap()
    }

    #[test]
    fn usage_success_preserves_status_headers_and_json() {
        let base = test_http::serve_once(200, &[("x-test-quota", "42")], r#"{"plan":"plus"}"#);
        let response = client(&base)
            .fetch_usage("secret-token", Some("account-id"))
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            response.headers.get("x-test-quota").map(String::as_str),
            Some("42")
        );
        assert_eq!(response.body["plan"], "plus");
    }

    #[test]
    fn malformed_success_body_is_rejected_without_exposing_it() {
        let base = test_http::serve_once(200, &[], "secret-token: not-json");
        let error = client(&base).fetch_usage("secret-token", None).unwrap_err();

        assert!(matches!(error, CodexError::InvalidResponse));
        assert!(!error.to_string().contains("secret-token"));
    }

    #[test]
    fn refresh_maps_expired_login_and_rate_limits() {
        let expired =
            test_http::serve_once(401, &[], r#"{"error":{"code":"refresh_token_expired"}}"#);
        assert!(matches!(
            client(&expired).refresh_token("secret-refresh"),
            Err(CodexError::SessionExpired)
        ));

        let limited = test_http::serve_once(429, &[], r#"{"error":"rate_limited"}"#);
        assert!(matches!(
            client(&limited).refresh_token("secret-refresh"),
            Err(CodexError::RequestFailed(429))
        ));
    }

    #[test]
    fn request_timeout_becomes_a_safe_connection_error() {
        let base =
            test_http::serve_once_after(Duration::from_millis(80), 200, &[], r#"{"plan":"plus"}"#);
        let client = CodexClient::with_endpoints(
            &format!("{base}/usage"),
            &format!("{base}/token"),
            Duration::from_millis(10),
        )
        .unwrap();
        let error = client.fetch_usage("secret-token", None).unwrap_err();

        assert!(matches!(error, CodexError::ConnectionFailed));
        assert!(!error.to_string().contains("secret-token"));
    }
}
