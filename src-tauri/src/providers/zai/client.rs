use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::ZaiError;

const SUBSCRIPTION_URL: &str = "https://api.z.ai/api/biz/subscription/list";
const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";

#[derive(Debug)]
pub struct ZaiResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct ZaiClient {
    client: Client,
    subscription_url: String,
    quota_url: String,
}

impl ZaiClient {
    pub fn new() -> Result<Self, ZaiError> {
        Self::with_endpoints(SUBSCRIPTION_URL, QUOTA_URL, Duration::from_secs(15))
    }

    fn with_endpoints(
        subscription_url: &str,
        quota_url: &str,
        timeout: Duration,
    ) -> Result<Self, ZaiError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ZaiError::ConnectionFailed)?;
        Ok(Self {
            client,
            subscription_url: subscription_url.to_owned(),
            quota_url: quota_url.to_owned(),
        })
    }

    pub fn fetch_quota(&self, api_key: &str) -> Result<ZaiResponse, ZaiError> {
        self.fetch(&self.quota_url, api_key, "quota")
    }

    pub fn fetch_subscription(&self, api_key: &str) -> Result<ZaiResponse, ZaiError> {
        self.fetch(&self.subscription_url, api_key, "subscription")
    }

    fn fetch(&self, url: &str, api_key: &str, endpoint: &str) -> Result<ZaiResponse, ZaiError> {
        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "zai {endpoint} request failed (transport)");
                ZaiError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "zai {endpoint} HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response.text().map_err(|_| ZaiError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(ZaiResponse { status, body })
    }
}

#[cfg(test)]
impl ZaiClient {
    pub fn for_test(subscription_url: &str, quota_url: &str, timeout: Duration) -> Self {
        Self::with_endpoints(subscription_url, quota_url, timeout).unwrap()
    }
}
