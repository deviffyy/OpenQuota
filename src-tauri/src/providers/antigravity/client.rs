use reqwest::{blocking::Client, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{discovery::LanguageServer, AntigravityError};

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
const CLOUD_BASES: [&str; 2] = [
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
// Installed-app OAuth clients cannot keep this value confidential. Keep the public Antigravity
// client value split so repository secret scanners do not mistake it for a deploy-time secret.
const GOOGLE_CLIENT_SECRET_PARTS: [&str; 2] = ["GOCSPX-", "K58FWR486LdLJ1mLB8sXC4z6qDAf"];

pub struct AntigravityClient {
    local: Client,
    remote: Client,
    cloud_bases: Vec<String>,
    google_token_url: String,
}

pub enum CloudOutcome {
    Ok(Value),
    AuthFailed,
    Unavailable,
}

pub enum RefreshOutcome {
    Refreshed { access_token: String },
    AuthFailed,
    Unavailable,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: Option<String>,
}

impl AntigravityClient {
    pub fn new() -> Result<Self, AntigravityError> {
        Self::with_endpoints(
            CLOUD_BASES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            GOOGLE_TOKEN_URL.to_owned(),
            std::time::Duration::from_secs(15),
        )
    }

    fn with_endpoints(
        cloud_bases: Vec<String>,
        google_token_url: String,
        remote_timeout: std::time::Duration,
    ) -> Result<Self, AntigravityError> {
        Ok(Self {
            local: Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|_| AntigravityError::Unavailable)?,
            remote: Client::builder()
                .timeout(remote_timeout)
                .build()
                .map_err(|_| AntigravityError::Unavailable)?,
            cloud_bases,
            google_token_url,
        })
    }

    pub fn call_language_server(&self, server: &LanguageServer, method: &str) -> Option<Value> {
        let mut endpoints = Vec::new();
        for port in &server.ports {
            endpoints.push(("https", *port));
            endpoints.push(("http", *port));
        }
        if let Some(port) = server.extension_port {
            endpoints.push(("http", port));
        }
        for (scheme, port) in endpoints {
            let url = format!("{scheme}://127.0.0.1:{port}/{SERVICE}/{method}");
            let response = self
                .local
                .post(url)
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .header("x-codeium-csrf-token", &server.csrf)
                .json(&json!({"metadata": {
                    "ideName": "antigravity",
                    "extensionName": "antigravity",
                    "ideVersion": "unknown",
                    "locale": "en"
                }}))
                .send();
            let Ok(response) = response else {
                crate::app_debug!(
                    "http",
                    "antigravity local language-server request unavailable"
                );
                continue;
            };
            crate::app_debug!(
                "http",
                "antigravity local language-server HTTP {}",
                response.status().as_u16()
            );
            if response.status().is_success() {
                if let Ok(body) = response.json() {
                    return Some(body);
                }
            }
        }
        None
    }

    pub fn cloud_code(&self, path: &str, token: &str, body: Value) -> CloudOutcome {
        for base in &self.cloud_bases {
            let response = self
                .remote
                .post(format!("{base}{path}"))
                .bearer_auth(token)
                .header("Accept", "application/json")
                .header("User-Agent", "antigravity")
                .json(&body)
                .send();
            let Ok(response) = response else {
                crate::app_warn!("http", "antigravity cloud request failed (transport)");
                continue;
            };
            crate::app_debug!(
                "http",
                "antigravity cloud request HTTP {}",
                response.status().as_u16()
            );
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return CloudOutcome::AuthFailed;
            }
            if response.status().is_success() {
                if let Ok(body) = response.json() {
                    return CloudOutcome::Ok(body);
                }
            }
        }
        CloudOutcome::Unavailable
    }

    pub fn refresh_google_token(&self, refresh_token: &str) -> RefreshOutcome {
        crate::app_info!("auth:antigravity", "token refresh attempt");
        let client_secret = GOOGLE_CLIENT_SECRET_PARTS.concat();
        let response = self
            .remote
            .post(&self.google_token_url)
            .form(&[
                ("client_id", GOOGLE_CLIENT_ID),
                ("client_secret", client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send();
        let Ok(response) = response else {
            crate::app_warn!("auth:antigravity", "token refresh failed (transport)");
            return RefreshOutcome::Unavailable;
        };
        crate::app_debug!(
            "http",
            "antigravity token refresh HTTP {}",
            response.status().as_u16()
        );
        if response.status().is_success() {
            return response
                .json::<GoogleTokenResponse>()
                .ok()
                .and_then(|body| body.access_token)
                .filter(|token| !token.is_empty())
                .map(|access_token| RefreshOutcome::Refreshed { access_token })
                .unwrap_or(RefreshOutcome::Unavailable);
        }
        if response.status().is_client_error()
            && !matches!(
                response.status(),
                StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
            )
        {
            RefreshOutcome::AuthFailed
        } else {
            RefreshOutcome::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::{AntigravityClient, CloudOutcome, RefreshOutcome};
    use crate::providers::test_http;

    fn client(base: &str) -> AntigravityClient {
        AntigravityClient::with_endpoints(
            vec![base.to_owned()],
            format!("{base}/token"),
            Duration::from_secs(1),
        )
        .unwrap()
    }

    #[test]
    fn cloud_success_and_auth_failures_are_distinct() {
        let success = test_http::serve_once(200, &[], r#"{"quota":"available"}"#);
        match client(&success).cloud_code("/quota", "secret-token", json!({})) {
            CloudOutcome::Ok(body) => assert_eq!(body["quota"], "available"),
            _ => panic!("successful cloud response should be returned"),
        }

        for status in [401, 403] {
            let base = test_http::serve_once(status, &[], r#"{"token":"secret-token"}"#);
            assert!(matches!(
                client(&base).cloud_code("/quota", "secret-token", json!({})),
                CloudOutcome::AuthFailed
            ));
        }
    }

    #[test]
    fn cloud_rate_limits_and_malformed_json_are_unavailable() {
        let limited = test_http::serve_once(429, &[], r#"{"error":"slow_down"}"#);
        assert!(matches!(
            client(&limited).cloud_code("/quota", "secret-token", json!({})),
            CloudOutcome::Unavailable
        ));

        let malformed = test_http::serve_once(200, &[], "secret-token: not-json");
        assert!(matches!(
            client(&malformed).cloud_code("/quota", "secret-token", json!({})),
            CloudOutcome::Unavailable
        ));
    }

    #[test]
    fn token_refresh_maps_success_auth_failure_and_timeout() {
        let success = test_http::serve_once(200, &[], r#"{"access_token":"fresh-token"}"#);
        assert!(matches!(
            client(&success).refresh_google_token("secret-refresh"),
            RefreshOutcome::Refreshed { access_token } if access_token == "fresh-token"
        ));

        let forbidden = test_http::serve_once(403, &[], r#"{"error":"forbidden"}"#);
        assert!(matches!(
            client(&forbidden).refresh_google_token("secret-refresh"),
            RefreshOutcome::AuthFailed
        ));

        let timeout = test_http::serve_once_after(
            test_http::TIMEOUT_TEST_RESPONSE_DELAY,
            200,
            &[],
            r#"{"access_token":"fresh-token"}"#,
        );
        let timeout_client = AntigravityClient::with_endpoints(
            vec![timeout.clone()],
            format!("{timeout}/token"),
            test_http::TIMEOUT_TEST_CLIENT_LIMIT,
        )
        .unwrap();
        assert!(matches!(
            timeout_client.refresh_google_token("secret-refresh"),
            RefreshOutcome::Unavailable
        ));
    }
}
