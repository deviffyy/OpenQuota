use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::KimiError;

const USAGES_URL: &str = "https://api.kimi.com/coding/v1/usages";

#[derive(Debug)]
pub struct EndpointResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct KimiClient {
    client: Client,
    url: String,
}

impl KimiClient {
    pub fn new() -> Result<Self, KimiError> {
        Self::with_endpoint(USAGES_URL, Duration::from_secs(15))
    }

    fn with_endpoint(url: &str, timeout: Duration) -> Result<Self, KimiError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| KimiError::ConnectionFailed)?;
        Ok(Self {
            client,
            url: url.to_owned(),
        })
    }

    pub fn fetch(&self, api_key: &str) -> Result<EndpointResponse, KimiError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(&self.url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "kimi usages request failed (transport)");
                KimiError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "kimi usages HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response.text().map_err(|_| KimiError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(EndpointResponse { status, body })
    }
}

#[cfg(test)]
impl KimiClient {
    pub fn for_test(url: &str, timeout: Duration) -> Self {
        Self::with_endpoint(url, timeout).unwrap()
    }
}
