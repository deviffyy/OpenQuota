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
}

impl CodexClient {
    pub fn new() -> Result<Self, CodexError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| CodexError::ConnectionFailed)?;
        Ok(Self { client })
    }

    pub fn fetch_usage(
        &self,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<UsageResponse, CodexError> {
        let mut request = self
            .client
            .get(USAGE_URL)
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
            .post(REFRESH_URL)
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
