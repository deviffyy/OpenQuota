use std::time::Duration;

use reqwest::{blocking::Client, StatusCode, Url};
use serde_json::Value;

use super::{auth::DevinAuth, DevinError};

const CLOUD_SERVICE: &str = "exa.seat_management_pb.SeatManagementService";
pub(super) const CLOUD_COMPAT_VERSION: &str = "1.108.2";

pub(super) struct DevinResponse {
    pub status: StatusCode,
    pub body: Value,
}

pub(super) struct DevinClient {
    client: Client,
}

impl DevinClient {
    pub fn new() -> Result<Self, DevinError> {
        Self::with_timeout(Duration::from_secs(15))
    }

    fn with_timeout(timeout: Duration) -> Result<Self, DevinError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(timeout)
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| DevinError::ConnectionFailed)?;
        Ok(Self { client })
    }

    pub fn fetch_user_status(&self, auth: &DevinAuth) -> Result<DevinResponse, DevinError> {
        let url = user_status_url(auth.effective_api_server_url())?;
        let started = std::time::Instant::now();
        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .json(&serde_json::json!({
                "metadata": {
                    "apiKey": auth.api_key.as_str(),
                    "ideName": "devin",
                    "ideVersion": CLOUD_COMPAT_VERSION,
                    "extensionName": "devin",
                    "extensionVersion": CLOUD_COMPAT_VERSION,
                    "locale": "en"
                }
            }))
            .send()
            .map_err(|_| {
                crate::app_warn!("http", "devin user-status request failed (transport)");
                DevinError::ConnectionFailed
            })?;
        let status = response.status();
        crate::app_debug!(
            "http",
            "devin user-status HTTP {} ({}ms)",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        let text = response.text().map_err(|_| DevinError::InvalidResponse)?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(DevinResponse { status, body })
    }
}

fn user_status_url(api_server_url: &str) -> Result<Url, DevinError> {
    let base = api_server_url.trim_end_matches('/');
    Url::parse(&format!("{base}/{CLOUD_SERVICE}/GetUserStatus"))
        .map_err(|_| DevinError::InvalidResponse)
}

#[cfg(test)]
impl DevinClient {
    pub fn for_test(timeout: Duration) -> Self {
        Self::with_timeout(timeout).unwrap()
    }
}
