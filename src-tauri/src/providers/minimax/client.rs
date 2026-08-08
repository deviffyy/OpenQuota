use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::MiniMaxError;

const REMAINS_URL: &str = "https://www.minimax.io/v1/token_plan/remains";

#[derive(Debug)]
pub struct EndpointResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct MiniMaxClient {
    client: Client,
    url: String,
}

impl MiniMaxClient {
    pub fn new() -> Result<Self, MiniMaxError> {
        Self::with_endpoint(REMAINS_URL, Duration::from_secs(15))
    }

    fn with_endpoint(url: &str, timeout: Duration) -> Result<Self, MiniMaxError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| MiniMaxError::ConnectionFailed)?;
        Ok(Self {
            client,
            url: url.to_owned(),
        })
    }

    pub fn fetch(&self, api_key: &str) -> Result<EndpointResponse, MiniMaxError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(&self.url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "minimax token_plan request failed (transport)");
                MiniMaxError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "minimax token_plan HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response.text().map_err(|_| MiniMaxError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(EndpointResponse { status, body })
    }
}

#[cfg(test)]
impl MiniMaxClient {
    pub fn for_test(url: &str, timeout: Duration) -> Self {
        Self::with_endpoint(url, timeout).unwrap()
    }
}
