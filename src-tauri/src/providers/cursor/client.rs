use std::time::Duration;

use reqwest::{blocking::Client, StatusCode, Url};

use super::{auth::token_subject, CursorError};

const USAGE_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
const PLAN_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo";
const CREDITS_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCreditGrantsBalance";
const REFRESH_URL: &str = "https://api2.cursor.sh/oauth/token";
const REST_USAGE_URL: &str = "https://cursor.com/api/usage";
const STRIPE_URL: &str = "https://cursor.com/api/auth/stripe";
const CSV_URL: &str = "https://cursor.com/api/dashboard/export-usage-events-csv";
const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

#[derive(Debug)]
pub struct CursorResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl CursorResponse {
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorSession {
    pub user_id: String,
    pub session_token: String,
}

#[derive(Clone)]
pub(super) struct Endpoints {
    pub usage: String,
    pub plan: String,
    pub credits: String,
    pub refresh: String,
    pub rest_usage: String,
    pub stripe: String,
    pub csv: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            usage: USAGE_URL.into(),
            plan: PLAN_URL.into(),
            credits: CREDITS_URL.into(),
            refresh: REFRESH_URL.into(),
            rest_usage: REST_USAGE_URL.into(),
            stripe: STRIPE_URL.into(),
            csv: CSV_URL.into(),
        }
    }
}

pub struct CursorClient {
    client: Client,
    endpoints: Endpoints,
}

impl CursorClient {
    pub fn new() -> Result<Self, CursorError> {
        Self::with_endpoints(Endpoints::default())
    }

    pub(super) fn with_endpoints(endpoints: Endpoints) -> Result<Self, CursorError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("OpenQuota/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| CursorError::ConnectionFailed)?;
        Ok(Self { client, endpoints })
    }

    pub fn fetch_usage(&self, access_token: &str) -> Result<CursorResponse, CursorError> {
        self.connect_post(&self.endpoints.usage, access_token)
    }

    pub fn fetch_plan(&self, access_token: &str) -> Result<CursorResponse, CursorError> {
        self.connect_post(&self.endpoints.plan, access_token)
    }

    pub fn fetch_credits(&self, access_token: &str) -> Result<CursorResponse, CursorError> {
        self.connect_post(&self.endpoints.credits, access_token)
    }

    pub fn refresh_token(&self, refresh_token: &str) -> Result<CursorResponse, CursorError> {
        self.send(
            self.client
                .post(&self.endpoints.refresh)
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "grant_type": "refresh_token",
                    "client_id": CLIENT_ID,
                    "refresh_token": refresh_token,
                }))
                .timeout(Duration::from_secs(15)),
        )
    }

    pub fn fetch_request_usage(
        &self,
        access_token: &str,
    ) -> Result<Option<CursorResponse>, CursorError> {
        let Some(session) = session(access_token) else {
            return Ok(None);
        };
        let mut url =
            Url::parse(&self.endpoints.rest_usage).map_err(|_| CursorError::InvalidResponse)?;
        url.query_pairs_mut().append_pair("user", &session.user_id);
        self.send(
            self.client
                .get(url)
                .header(
                    "Cookie",
                    format!("WorkosCursorSessionToken={}", session.session_token),
                )
                .timeout(Duration::from_secs(10)),
        )
        .map(Some)
    }

    pub fn fetch_stripe_balance(
        &self,
        access_token: &str,
    ) -> Result<Option<CursorResponse>, CursorError> {
        let Some(session) = session(access_token) else {
            return Ok(None);
        };
        self.send(
            self.client
                .get(&self.endpoints.stripe)
                .header(
                    "Cookie",
                    format!("WorkosCursorSessionToken={}", session.session_token),
                )
                .timeout(Duration::from_secs(10)),
        )
        .map(Some)
    }

    pub fn fetch_usage_csv(
        &self,
        access_token: &str,
        start_millis: i64,
        end_millis: i64,
    ) -> Result<Option<CursorResponse>, CursorError> {
        let Some(session) = session(access_token) else {
            return Ok(None);
        };
        let mut url = Url::parse(&self.endpoints.csv).map_err(|_| CursorError::InvalidResponse)?;
        url.query_pairs_mut()
            .append_pair("startDate", &start_millis.to_string())
            .append_pair("endDate", &end_millis.to_string())
            .append_pair("strategy", "tokens");
        self.send(
            self.client
                .get(url)
                .header(
                    "Cookie",
                    format!("WorkosCursorSessionToken={}", session.session_token),
                )
                .header("Accept", "text/csv")
                .timeout(Duration::from_secs(30)),
        )
        .map(Some)
    }

    fn connect_post(&self, url: &str, access_token: &str) -> Result<CursorResponse, CursorError> {
        self.send(
            self.client
                .post(url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .body("{}")
                .timeout(Duration::from_secs(10)),
        )
    }

    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<CursorResponse, CursorError> {
        let response = request.send().map_err(|_| CursorError::ConnectionFailed)?;
        let status = response.status();
        let body = response
            .bytes()
            .map_err(|_| CursorError::InvalidResponse)?
            .to_vec();
        Ok(CursorResponse { status, body })
    }
}

pub fn session(access_token: &str) -> Option<CursorSession> {
    let subject = token_subject(Some(access_token))?;
    let parts = subject.split('|').collect::<Vec<_>>();
    let user_id = parts.get(1).copied().unwrap_or(parts[0]).trim();
    if user_id.is_empty() {
        return None;
    }
    Some(CursorSession {
        user_id: user_id.into(),
        session_token: format!("{user_id}%3A%3A{access_token}"),
    })
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    use super::*;
    use crate::providers::test_http;

    fn jwt() -> String {
        let payload =
            URL_SAFE_NO_PAD.encode(r#"{"sub":"google-oauth2|user_abc123","exp":9999999999}"#);
        format!("a.{payload}.c")
    }

    fn client(base: &str) -> CursorClient {
        CursorClient::with_endpoints(Endpoints {
            usage: format!("{base}/usage"),
            plan: format!("{base}/plan"),
            credits: format!("{base}/credits"),
            refresh: format!("{base}/token"),
            rest_usage: format!("{base}/rest"),
            stripe: format!("{base}/stripe"),
            csv: format!("{base}/csv"),
        })
        .unwrap()
    }

    #[test]
    fn session_uses_second_subject_component_and_encoded_separator() {
        let token = jwt();
        let session = session(&token).unwrap();
        assert_eq!(session.user_id, "user_abc123");
        assert_eq!(session.session_token, format!("user_abc123%3A%3A{token}"));
    }

    #[test]
    fn connect_success_returns_status_and_json_body() {
        let base = test_http::serve_once(200, &[], r#"{"enabled":true}"#);
        let response = client(&base).fetch_usage("secret-access").unwrap();
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.json().unwrap()["enabled"], true);
    }

    #[test]
    fn csv_without_jwt_session_skips_network() {
        assert!(client("http://127.0.0.1:1")
            .fetch_usage_csv("not-a-jwt", 1_000_000, 2_000_000)
            .unwrap()
            .is_none());
    }
}
