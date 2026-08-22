use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::OpenCodeError;

const GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

#[derive(Debug)]
pub(super) struct UsageResponse {
    pub(super) status: StatusCode,
    pub(super) body: Value,
}

pub(super) struct OpenCodeClient {
    client: Client,
    usage_url: String,
}

impl OpenCodeClient {
    pub(super) fn new() -> Result<Self, OpenCodeError> {
        Self::with_endpoint(GO_USAGE_URL, Duration::from_secs(15))
    }

    fn with_endpoint(usage_url: &str, timeout: Duration) -> Result<Self, OpenCodeError> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(timeout)
                .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| OpenCodeError::ConnectionFailed)?,
            usage_url: usage_url.into(),
        })
    }

    pub(super) fn fetch_go_usage(&self, api_key: &str) -> Result<UsageResponse, OpenCodeError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(&self.usage_url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "opencode go usage request failed (transport)");
                OpenCodeError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "opencode go usage HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let body = response
            .text()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        Ok(UsageResponse { status, body })
    }
}

#[cfg(test)]
impl OpenCodeClient {
    pub(super) fn for_test(url: &str, timeout: Duration) -> Self {
        Self::with_endpoint(url, timeout).expect("test OpenCode endpoint should be valid")
    }
}
