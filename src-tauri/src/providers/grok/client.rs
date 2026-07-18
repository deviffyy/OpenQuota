use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use super::GrokError;

const CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";
const TOKEN_AUTH_HEADER: &str = "xai-grok-cli";

#[derive(Debug)]
pub struct GrokResponse {
    pub status: StatusCode,
    pub body: Value,
}

#[derive(Debug, Deserialize)]
pub struct TokenRefresh {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_in: Option<f64>,
}

pub struct GrokClient {
    client: Client,
    credits_url: String,
    settings_url: String,
    refresh_url: String,
}

impl GrokClient {
    pub fn new() -> Result<Self, GrokError> {
        Self::with_endpoints(
            CREDITS_URL,
            SETTINGS_URL,
            REFRESH_URL,
            Duration::from_secs(15),
        )
    }

    fn with_endpoints(
        credits_url: &str,
        settings_url: &str,
        refresh_url: &str,
        timeout: Duration,
    ) -> Result<Self, GrokError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| GrokError::ConnectionFailed)?;
        Ok(Self {
            client,
            credits_url: credits_url.to_owned(),
            settings_url: settings_url.to_owned(),
            refresh_url: refresh_url.to_owned(),
        })
    }

    pub fn fetch_credits(&self, access_token: &str) -> Result<GrokResponse, GrokError> {
        self.fetch_authenticated(&self.credits_url, access_token, "billing")
    }

    pub fn fetch_settings(&self, access_token: &str) -> Result<GrokResponse, GrokError> {
        self.fetch_authenticated(&self.settings_url, access_token, "settings")
    }

    fn fetch_authenticated(
        &self,
        url: &str,
        access_token: &str,
        endpoint: &str,
    ) -> Result<GrokResponse, GrokError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token.trim())
            .header("X-XAI-Token-Auth", TOKEN_AUTH_HEADER)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "grok {endpoint} request failed (transport)");
                GrokError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "grok {endpoint} HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response.text().map_err(|_| GrokError::InvalidResponse)?;
        Ok(GrokResponse {
            status,
            body: serde_json::from_str(&text).unwrap_or(Value::Null),
        })
    }

    pub fn refresh_token(
        &self,
        refresh_token: &str,
        client_id: &str,
    ) -> Result<TokenRefresh, GrokError> {
        let started = std::time::Instant::now();
        crate::app_info!("auth:grok", "token refresh attempt");
        let response = self
            .client
            .post(&self.refresh_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("refresh_token", refresh_token),
            ])
            .send()
            .map_err(|_| {
                crate::app_warn!("auth:grok", "token refresh failed (transport)");
                GrokError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "grok token refresh HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        if !status.is_success() {
            return Err(
                if matches!(
                    status,
                    StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
                ) {
                    GrokError::Expired
                } else {
                    GrokError::RequestFailed(status.as_u16())
                },
            );
        }
        let mut refreshed: TokenRefresh =
            response.json().map_err(|_| GrokError::InvalidResponse)?;
        refreshed.access_token = refreshed.access_token.trim().to_owned();
        if refreshed.access_token.is_empty() {
            return Err(GrokError::Expired);
        }
        crate::app_info!("auth:grok", "token refresh succeeded");
        Ok(refreshed)
    }

    #[cfg(test)]
    pub fn for_test(
        credits_url: &str,
        settings_url: &str,
        refresh_url: &str,
        timeout: Duration,
    ) -> Self {
        Self::with_endpoints(credits_url, settings_url, refresh_url, timeout).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use super::GrokClient;
    use crate::providers::{grok::GrokError, test_http};

    fn capture_once(
        response_body: &str,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response_body = response_body.to_owned();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0_u8; 1024];
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                let text = String::from_utf8_lossy(&request);
                let Some(header_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let content_length = text[..header_end]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), receiver, handle)
    }

    #[test]
    fn authenticated_fetch_uses_cli_headers_without_leaking_the_token() {
        let (url, request, handle) = capture_once(r#"{"config":{}}"#);
        let client = GrokClient::for_test(&url, &url, &url, Duration::from_secs(1));

        let response = client.fetch_credits(" secret-token ").unwrap();

        assert!(response.body.get("config").is_some());
        let request = request.recv_timeout(Duration::from_secs(1)).unwrap();
        let request_lower = request.to_ascii_lowercase();
        assert!(request_lower.contains("authorization: bearer secret-token"));
        assert!(request_lower.contains("x-xai-token-auth: xai-grok-cli"));
        assert!(request_lower.contains("accept: application/json"));
        handle.join().unwrap();
    }

    #[test]
    fn refresh_form_encodes_reserved_characters() {
        let (url, request, handle) = capture_once(r#"{"access_token":"new","expires_in":3600}"#);
        let client = GrokClient::for_test(&url, &url, &url, Duration::from_secs(1));

        let refreshed = client
            .refresh_token("refresh token&=+/?%", "client id&=+/?%")
            .unwrap();

        assert_eq!(refreshed.access_token, "new");
        let request = request.recv_timeout(Duration::from_secs(1)).unwrap();
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("client_id=client+id%26%3D%2B%2F%3F%25"));
        assert!(body.contains("refresh_token=refresh+token%26%3D%2B%2F%3F%25"));
        handle.join().unwrap();
    }

    #[test]
    fn refresh_auth_failure_and_transport_timeout_are_typed() {
        let unauthorized = test_http::serve_once(401, &[], "{}");
        let client = GrokClient::for_test(
            &unauthorized,
            &unauthorized,
            &unauthorized,
            Duration::from_secs(1),
        );
        assert!(matches!(
            client.refresh_token("refresh", "client"),
            Err(GrokError::Expired)
        ));

        let delayed = test_http::serve_once_after(
            test_http::TIMEOUT_TEST_RESPONSE_DELAY,
            200,
            &[],
            r#"{"config":{}}"#,
        );
        let client = GrokClient::for_test(
            &delayed,
            &delayed,
            &delayed,
            test_http::TIMEOUT_TEST_CLIENT_LIMIT,
        );
        assert!(matches!(
            client.fetch_credits("secret"),
            Err(GrokError::ConnectionFailed)
        ));
    }
}
