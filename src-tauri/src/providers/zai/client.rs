use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

use super::ZaiError;

const GLOBAL_SUBSCRIPTION_URL: &str = "https://api.z.ai/api/biz/subscription/list";
const GLOBAL_QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const CHINA_SUBSCRIPTION_URL: &str = "https://open.bigmodel.cn/api/biz/subscription/list";
const CHINA_QUOTA_URL: &str = "https://open.bigmodel.cn/api/monitor/usage/quota/limit";

#[derive(Clone, Copy)]
enum Authentication {
    Bearer,
    Raw,
}

#[derive(Debug)]
pub struct ZaiResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub struct ZaiClient {
    client: Client,
    subscription_url: String,
    quota_url: String,
    authentication: Authentication,
}

impl ZaiClient {
    pub fn global() -> Result<Self, ZaiError> {
        Self::with_endpoints(
            GLOBAL_SUBSCRIPTION_URL,
            GLOBAL_QUOTA_URL,
            Authentication::Bearer,
            Duration::from_secs(15),
        )
    }

    pub fn china() -> Result<Self, ZaiError> {
        Self::with_endpoints(
            CHINA_SUBSCRIPTION_URL,
            CHINA_QUOTA_URL,
            Authentication::Raw,
            Duration::from_secs(15),
        )
    }

    fn with_endpoints(
        subscription_url: &str,
        quota_url: &str,
        authentication: Authentication,
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
            authentication,
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
        let request = self.client.get(url).header("Accept", "application/json");
        let request = match self.authentication {
            Authentication::Bearer => request.bearer_auth(api_key),
            Authentication::Raw => request.header("Authorization", api_key),
        };
        let response = request.send().map_err(|_| {
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
        Self::with_endpoints(subscription_url, quota_url, Authentication::Bearer, timeout).unwrap()
    }

    pub fn china_for_test(subscription_url: &str, quota_url: &str, timeout: Duration) -> Self {
        Self::with_endpoints(subscription_url, quota_url, Authentication::Raw, timeout).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::providers::test_http;

    use super::ZaiClient;

    #[test]
    fn global_and_china_services_use_their_respective_authorization_schemes() {
        let (global_url, global_request) = test_http::serve_once_recording(200, "{}");
        let global = ZaiClient::for_test(&global_url, &global_url, Duration::from_secs(1));
        global.fetch_quota("test-key").unwrap();
        assert!(global_request
            .recv()
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer test-key"));

        let (china_url, china_request) = test_http::serve_once_recording(200, "{}");
        let china = ZaiClient::china_for_test(&china_url, &china_url, Duration::from_secs(1));
        china.fetch_quota("test-key").unwrap();
        let request = china_request.recv().unwrap().to_ascii_lowercase();
        assert!(request.contains("authorization: test-key"));
        assert!(!request.contains("authorization: bearer"));
    }
}
