use chrono::{DateTime, Utc};
use reqwest::{blocking::Client, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{auth::ClaudeOAuthConfig, ClaudeError};

const OAUTH_SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

#[derive(Debug, Deserialize)]
pub struct ClaudeRefreshResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<f64>,
}

pub struct ClaudeClient {
    client: Client,
}

impl ClaudeClient {
    pub fn new() -> Result<Self, ClaudeError> {
        Self::with_timeout(std::time::Duration::from_secs(15))
    }

    fn with_timeout(timeout: std::time::Duration) -> Result<Self, ClaudeError> {
        Ok(Self {
            client: Client::builder()
                .timeout(timeout)
                .build()
                .map_err(|_| ClaudeError::ConnectionFailed)?,
        })
    }

    pub fn fetch_usage(
        &self,
        token: &str,
        config: &ClaudeOAuthConfig,
    ) -> Result<(StatusCode, Value, Option<u64>), ClaudeError> {
        let response = self
            .client
            .get(&config.usage_url)
            .bearer_auth(token.trim())
            .header("Accept", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-code/2.1.69")
            .send()
            .map_err(|_| ClaudeError::ConnectionFailed)?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_retry_after(value, Utc::now()));
        let body = response.json().map_err(|_| ClaudeError::InvalidResponse)?;
        Ok((status, body, retry_after))
    }

    pub fn refresh_token(
        &self,
        token: &str,
        config: &ClaudeOAuthConfig,
    ) -> Result<ClaudeRefreshResponse, ClaudeError> {
        let response = self
            .client
            .post(&config.refresh_url)
            .json(&json!({
                "grant_type": "refresh_token",
                "refresh_token": token,
                "client_id": config.client_id,
                "scope": OAUTH_SCOPES
            }))
            .send()
            .map_err(|_| ClaudeError::ConnectionFailed)?;
        let status = response.status();
        if !status.is_success() {
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::BAD_REQUEST) {
                let body = response.json::<Value>().ok();
                let code = body.as_ref().and_then(|body| {
                    body.get("error")
                        .and_then(Value::as_str)
                        .or_else(|| body.get("error_description").and_then(Value::as_str))
                });
                if code == Some("invalid_grant") {
                    return Err(ClaudeError::SessionExpired);
                }
            }
            return Err(ClaudeError::RequestFailed(status.as_u16()));
        }
        let refreshed: ClaudeRefreshResponse =
            response.json().map_err(|_| ClaudeError::InvalidResponse)?;
        if refreshed.access_token.trim().is_empty() {
            return Err(ClaudeError::TokenExpired);
        }
        Ok(refreshed)
    }
}

fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<u64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let date = DateTime::parse_from_rfc2822(value).ok()?.to_utc();
    let milliseconds = date.signed_duration_since(now).num_milliseconds().max(0) as u64;
    Some(milliseconds.div_ceil(1000))
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use chrono::{TimeZone, Utc};

    use super::{parse_retry_after, ClaudeClient};
    use crate::providers::{
        claude::{auth::ClaudeOAuthConfig, ClaudeError},
        test_http,
    };

    fn config(base: &str) -> ClaudeOAuthConfig {
        ClaudeOAuthConfig {
            usage_url: format!("{base}/usage"),
            refresh_url: format!("{base}/token"),
            client_id: "test-client".into(),
        }
    }

    #[test]
    fn usage_success_reads_json_and_retry_after() {
        let base = test_http::serve_once(200, &[("retry-after", "120")], r#"{"plan":"max"}"#);
        let (status, body, retry_after) = ClaudeClient::new()
            .unwrap()
            .fetch_usage("secret-token", &config(&base))
            .unwrap();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["plan"], "max");
        assert_eq!(retry_after, Some(120));
    }

    #[test]
    fn malformed_usage_and_timeout_return_safe_errors() {
        let malformed = test_http::serve_once(200, &[], "secret-token: not-json");
        let error = ClaudeClient::new()
            .unwrap()
            .fetch_usage("secret-token", &config(&malformed))
            .unwrap_err();
        assert!(matches!(error, ClaudeError::InvalidResponse));
        assert!(!error.to_string().contains("secret-token"));

        let timeout = test_http::serve_once_after(
            test_http::TIMEOUT_TEST_RESPONSE_DELAY,
            200,
            &[],
            r#"{"plan":"max"}"#,
        );
        let error = ClaudeClient::with_timeout(test_http::TIMEOUT_TEST_CLIENT_LIMIT)
            .unwrap()
            .fetch_usage("secret-token", &config(&timeout))
            .unwrap_err();
        assert!(matches!(error, ClaudeError::ConnectionFailed));
        assert!(!error.to_string().contains("secret-token"));
    }

    #[test]
    fn refresh_distinguishes_auth_failures_and_rate_limits() {
        let forbidden = test_http::serve_once(400, &[], r#"{"error":"invalid_grant"}"#);
        assert!(matches!(
            ClaudeClient::new()
                .unwrap()
                .refresh_token("secret-refresh", &config(&forbidden)),
            Err(ClaudeError::SessionExpired)
        ));

        let limited = test_http::serve_once(429, &[], r#"{"error":"slow_down"}"#);
        assert!(matches!(
            ClaudeClient::new()
                .unwrap()
                .refresh_token("secret-refresh", &config(&limited)),
            Err(ClaudeError::RequestFailed(429))
        ));
    }

    #[test]
    fn refresh_does_not_misclassify_an_unrecognized_400_as_expired() {
        let base = test_http::serve_once(400, &[], "<html>Bad Gateway</html>");
        assert!(matches!(
            ClaudeClient::new()
                .unwrap()
                .refresh_token("secret-refresh", &config(&base)),
            Err(ClaudeError::RequestFailed(400))
        ));
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let now = Utc.with_ymd_and_hms(2015, 10, 21, 7, 27, 0).unwrap();
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT", now),
            Some(60)
        );
    }
}
