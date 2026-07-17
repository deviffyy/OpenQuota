use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::OpenRouterError;

const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";

#[derive(Debug)]
pub struct EndpointResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct OpenRouterClient {
    client: Client,
    credits_url: String,
    key_url: String,
}

impl OpenRouterClient {
    pub fn new() -> Result<Self, OpenRouterError> {
        Self::with_endpoints(CREDITS_URL, KEY_URL, Duration::from_secs(15))
    }

    fn with_endpoints(
        credits_url: &str,
        key_url: &str,
        timeout: Duration,
    ) -> Result<Self, OpenRouterError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| OpenRouterError::ConnectionFailed)?;
        Ok(Self {
            client,
            credits_url: credits_url.to_owned(),
            key_url: key_url.to_owned(),
        })
    }

    pub fn fetch_credits(&self, api_key: &str) -> Result<EndpointResponse, OpenRouterError> {
        self.fetch(&self.credits_url, api_key, "credits")
    }

    pub fn fetch_key(&self, api_key: &str) -> Result<EndpointResponse, OpenRouterError> {
        self.fetch(&self.key_url, api_key, "key")
    }

    fn fetch(
        &self,
        url: &str,
        api_key: &str,
        endpoint: &str,
    ) -> Result<EndpointResponse, OpenRouterError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "openrouter {endpoint} request failed (transport)");
                OpenRouterError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "openrouter {endpoint} HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response
            .text()
            .map_err(|_| OpenRouterError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(EndpointResponse { status, body })
    }
}

#[cfg(test)]
impl OpenRouterClient {
    pub fn for_test(credits_url: &str, key_url: &str, timeout: Duration) -> Self {
        Self::with_endpoints(credits_url, key_url, timeout).unwrap()
    }
}
