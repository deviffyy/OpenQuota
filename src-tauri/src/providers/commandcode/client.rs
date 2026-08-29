use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::CommandCodeError;

const CREDITS_URL: &str = "https://api.commandcode.ai/alpha/billing/credits";
const SUBSCRIPTION_URL: &str = "https://api.commandcode.ai/alpha/billing/subscriptions";

#[derive(Debug)]
pub struct EndpointResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct CommandCodeClient {
    client: Client,
    credits_url: String,
    subscription_url: String,
}

impl CommandCodeClient {
    pub fn new() -> Result<Self, CommandCodeError> {
        Self::with_endpoints(CREDITS_URL, SUBSCRIPTION_URL, Duration::from_secs(15))
    }

    fn with_endpoints(
        credits_url: &str,
        subscription_url: &str,
        timeout: Duration,
    ) -> Result<Self, CommandCodeError> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(timeout)
                .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| CommandCodeError::ConnectionFailed)?,
            credits_url: credits_url.into(),
            subscription_url: subscription_url.into(),
        })
    }

    pub fn fetch(
        &self,
        api_key: &str,
    ) -> Result<(EndpointResponse, EndpointResponse), CommandCodeError> {
        Ok((
            self.fetch_endpoint(&self.credits_url, api_key, "credits")?,
            self.fetch_endpoint(&self.subscription_url, api_key, "subscriptions")?,
        ))
    }

    fn fetch_endpoint(
        &self,
        url: &str,
        api_key: &str,
        endpoint: &str,
    ) -> Result<EndpointResponse, CommandCodeError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "command-code {endpoint} request failed (transport)");
                CommandCodeError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "command-code {endpoint} HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let body = response
            .text()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        Ok(EndpointResponse { status, body })
    }
}
