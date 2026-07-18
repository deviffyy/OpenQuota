use std::time::Duration;

use reqwest::{blocking::Client, StatusCode, Url};
use serde_json::Value;

use super::CopilotError;

const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";
const ORGS_URL: &str = "https://api.github.com/user/orgs?per_page=100";
const API_BASE_URL: &str = "https://api.github.com/";

#[derive(Debug)]
pub(super) struct CopilotResponse {
    pub(super) status: StatusCode,
    pub(super) body: Value,
}

pub(super) struct CopilotClient {
    client: Client,
    usage_url: String,
    orgs_url: String,
    api_base_url: String,
}

impl CopilotClient {
    pub(super) fn new() -> Result<Self, CopilotError> {
        Self::with_endpoints(USAGE_URL, ORGS_URL, API_BASE_URL, Duration::from_secs(15))
    }

    fn with_endpoints(
        usage_url: &str,
        orgs_url: &str,
        api_base_url: &str,
        timeout: Duration,
    ) -> Result<Self, CopilotError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .build()
            .map_err(|_| CopilotError::ConnectionFailed)?;
        Url::parse(usage_url).map_err(|_| CopilotError::InvalidResponse)?;
        Url::parse(orgs_url).map_err(|_| CopilotError::InvalidResponse)?;
        Url::parse(api_base_url).map_err(|_| CopilotError::InvalidResponse)?;
        Ok(Self {
            client,
            usage_url: usage_url.to_owned(),
            orgs_url: orgs_url.to_owned(),
            api_base_url: api_base_url.to_owned(),
        })
    }

    pub(super) fn fetch_usage(&self, token: &str) -> Result<CopilotResponse, CopilotError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(&self.usage_url)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/json")
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .header("X-GitHub-Api-Version", "2025-04-01")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "copilot usage request failed (transport)");
                CopilotError::ConnectionFailed
            })?;
        crate::app_debug!(
            "http",
            "copilot usage HTTP {} ({}ms)",
            response.status().as_u16(),
            started.elapsed().as_millis()
        );
        response_body(response)
    }

    pub(super) fn fetch_orgs(
        &self,
        token: &str,
        timeout: Duration,
    ) -> Result<CopilotResponse, CopilotError> {
        self.fetch_org_endpoint(&self.orgs_url, token, "organization list", timeout)
    }

    pub(super) fn fetch_org_usage(
        &self,
        org: &str,
        token: &str,
        timeout: Duration,
    ) -> Result<CopilotResponse, CopilotError> {
        let mut url = Url::parse(&self.api_base_url).map_err(|_| CopilotError::InvalidResponse)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| CopilotError::InvalidResponse)?;
            segments
                .pop_if_empty()
                .push("orgs")
                .push(org)
                .push("settings")
                .push("billing")
                .push("usage")
                .push("summary");
        }
        self.fetch_org_endpoint(url.as_str(), token, "organization billing", timeout)
    }

    fn fetch_org_endpoint(
        &self,
        url: &str,
        token: &str,
        endpoint: &str,
        timeout: Duration,
    ) -> Result<CopilotResponse, CopilotError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/vnd.github+json")
            .header(
                "User-Agent",
                concat!("OpenQuota/", env!("CARGO_PKG_VERSION")),
            )
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(timeout)
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "copilot {endpoint} request failed (transport)");
                CopilotError::ConnectionFailed
            })?;
        crate::app_debug!(
            "http",
            "copilot {endpoint} HTTP {} ({}ms)",
            response.status().as_u16(),
            started.elapsed().as_millis()
        );
        response_body(response)
    }

    #[cfg(test)]
    pub(super) fn for_test(
        usage_url: &str,
        orgs_url: &str,
        api_base_url: &str,
        timeout: Duration,
    ) -> Self {
        Self::with_endpoints(usage_url, orgs_url, api_base_url, timeout).unwrap()
    }
}

fn response_body(response: reqwest::blocking::Response) -> Result<CopilotResponse, CopilotError> {
    let status = response.status();
    let text = response.text().map_err(|_| CopilotError::InvalidResponse)?;
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(CopilotResponse { status, body })
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

    use super::CopilotClient;
    use crate::providers::{copilot::CopilotError, test_http};

    fn capture_once(body: &str) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_owned();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0_u8; 1024];
                let count = stream.read(&mut chunk).unwrap_or(0);
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (format!("http://{address}"), receiver, handle)
    }

    #[test]
    fn usage_request_matches_the_copilot_client_contract_without_logging_secrets() {
        let (url, request, handle) = capture_once("{}");
        let base = format!("{url}/");
        let client = CopilotClient::for_test(&url, &url, &base, Duration::from_secs(1));

        client.fetch_usage("secret-token").unwrap();

        let request = request.recv_timeout(Duration::from_secs(1)).unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("authorization: token secret-token"));
        assert!(lower.contains("editor-version: vscode/1.96.2"));
        assert!(lower.contains("editor-plugin-version: copilot-chat/0.26.7"));
        assert!(lower.contains("x-github-api-version: 2025-04-01"));
        handle.join().unwrap();
    }

    #[test]
    fn organization_slug_is_encoded_as_one_url_path_segment() {
        let (url, request, handle) = capture_once("{}");
        let base = format!("{url}/");
        let client = CopilotClient::for_test(&url, &url, &base, Duration::from_secs(1));

        client
            .fetch_org_usage("team/name", "secret-token", Duration::from_secs(1))
            .unwrap();

        let request = request.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            request.starts_with("GET /orgs/team%2Fname/settings/billing/usage/summary HTTP/1.1")
        );
        handle.join().unwrap();
    }

    #[test]
    fn transport_and_timeout_failures_are_safe_and_typed() {
        let client = CopilotClient::for_test(
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            "http://127.0.0.1:1/",
            Duration::from_millis(100),
        );
        let error = client.fetch_usage("super-secret-token").unwrap_err();
        assert!(matches!(error, CopilotError::ConnectionFailed));
        assert!(!error.to_string().contains("super-secret-token"));

        let delayed =
            test_http::serve_once_after(test_http::TIMEOUT_TEST_RESPONSE_DELAY, 200, &[], "{}");
        let client = CopilotClient::for_test(
            &delayed,
            &delayed,
            &format!("{delayed}/"),
            test_http::TIMEOUT_TEST_CLIENT_LIMIT,
        );
        let error = client.fetch_usage("another-secret-token").unwrap_err();
        assert!(matches!(error, CopilotError::ConnectionFailed));
        assert!(!error.to_string().contains("another-secret-token"));
    }
}
