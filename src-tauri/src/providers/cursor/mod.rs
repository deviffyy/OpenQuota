pub mod auth;
pub mod client;
pub mod csv;
pub mod mapper;

use std::sync::Arc;

use chrono::{Days, Local, TimeZone, Utc};
use reqwest::StatusCode;
use serde_json::Value;
use thiserror::Error;

use crate::{
    models::{
        MetricDefinition, MetricSection, ProviderDefinition, ProviderLink, ProviderSnapshot,
        UsageHistory, UsagePeriodSelection,
    },
    pricing::PricingStore,
};

use self::{
    auth::CursorAuthState,
    client::{CursorClient, CursorResponse},
    csv::parse_usage_csv,
    mapper::{
        map_live_usage, map_request_usage, request_fallback, stripe_balance_cents, usage_history,
        PlanUsageFacts,
    },
};

pub(crate) fn definition() -> ProviderDefinition {
    ProviderDefinition {
        id: "cursor".into(),
        display_name: "Cursor".into(),
        short_name: "Cu".into(),
        fallback_enabled: true,
        local_usage_source_note: Some("From your Cursor usage export".into()),
        links: vec![
            ProviderLink::new("Status", "https://status.cursor.com/"),
            ProviderLink::new("Dashboard", "https://www.cursor.com/dashboard"),
        ],
        metrics: vec![
            MetricDefinition::quota(
                "cursor.usage",
                "Total Usage",
                "usage",
                false,
                true,
                MetricSection::AlwaysVisible,
                false,
                "U",
            ),
            MetricDefinition::quota(
                "cursor.auto",
                "Auto Usage",
                "auto",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "A",
            ),
            MetricDefinition::quota(
                "cursor.api",
                "API Usage",
                "api",
                false,
                true,
                MetricSection::AlwaysVisible,
                true,
                "AP",
            ),
            MetricDefinition::quota_or_value(
                "cursor.onDemand",
                "Extra Usage",
                "onDemand",
                true,
                MetricSection::OnDemand,
                false,
                "E",
            ),
            MetricDefinition::quota(
                "cursor.requests",
                "Requests",
                "requests",
                false,
                false,
                MetricSection::OnDemand,
                false,
                "R",
            ),
            MetricDefinition::value(
                "cursor.credits",
                "Credits",
                "credits",
                false,
                MetricSection::OnDemand,
                false,
                "C",
                None,
            ),
            MetricDefinition::trend("cursor.trend"),
            MetricDefinition::usage(
                "cursor.today",
                "Today",
                UsagePeriodSelection::Today,
                MetricSection::OnDemand,
                "T",
            ),
            MetricDefinition::usage(
                "cursor.yesterday",
                "Yesterday",
                UsagePeriodSelection::Yesterday,
                MetricSection::OnDemand,
                "Y",
            ),
            MetricDefinition::usage(
                "cursor.last30",
                "Last 30 Days",
                UsagePeriodSelection::Last30Days,
                MetricSection::OnDemand,
                "M",
            ),
        ],
    }
}

#[derive(Debug, Error)]
pub enum CursorError {
    #[error("Not logged in. Sign in via Cursor app or run `agent login`.")]
    NotLoggedIn,
    #[error("Session expired. Sign in via Cursor app or run `agent login`.")]
    SessionExpired,
    #[error("Token expired. Sign in via Cursor app or run `agent login`.")]
    TokenExpired,
    #[error("The refreshed Cursor login could not be saved.")]
    AuthWrite,
    #[error("Could not connect to Cursor. Check your internet connection.")]
    ConnectionFailed,
    #[error("Cursor returned an invalid usage response.")]
    InvalidResponse,
    #[error("Cursor usage request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("Usage request failed after refresh. Try again.")]
    UsageAfterRefreshFailed,
    #[error("{0}")]
    RequestBasedUnavailable(String),
    #[error("Total usage limit missing from API response.")]
    TotalUsageLimitMissing,
    #[error("No active Cursor subscription.")]
    NoActiveSubscription,
}

pub struct CursorProvider {
    pricing: Arc<PricingStore>,
    client: CursorClient,
}

impl CursorProvider {
    pub fn new(pricing: Arc<PricingStore>) -> Result<Self, CursorError> {
        Ok(Self {
            pricing,
            client: CursorClient::new()?,
        })
    }

    pub fn refresh(&self) -> Result<ProviderSnapshot, CursorError> {
        let now = Utc::now();
        let auth = CursorAuthState::load()?.ok_or(CursorError::NotLoggedIn)?;
        self.refresh_with_auth(auth, now)
    }

    fn refresh_with_auth(
        &self,
        mut auth: CursorAuthState,
        now: chrono::DateTime<Utc>,
    ) -> Result<ProviderSnapshot, CursorError> {
        if auth.needs_refresh(now) {
            match self.refresh_access_token(&mut auth) {
                Ok(Some(_)) => {}
                Ok(None) if auth.access_token.is_none() => return Err(CursorError::NotLoggedIn),
                Err(error) if auth.access_token.is_none() => return Err(error),
                Ok(None) | Err(_) => {}
            }
        }
        let access_token = auth
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(CursorError::NotLoggedIn)?
            .to_owned();

        let usage_response = self.fetch_usage_with_retry(&access_token, &mut auth)?;
        require_success(&usage_response)?;
        let usage = json_object(&usage_response)?;
        let current_token = auth.access_token.as_deref().unwrap_or(&access_token);
        let (plan_name, plan_unavailable) = self.fetch_plan_name(current_token);

        if let Some(message) = request_fallback(&usage, plan_name.as_deref(), plan_unavailable) {
            let mapped = self.request_based_result(current_token, plan_name.as_deref(), message)?;
            return Ok(snapshot(mapped, UsageHistory::default(), Vec::new(), now));
        }
        if PlanUsageFacts::new(&usage).should_try_generic_request_fallback() {
            if let Ok(mapped) = self.request_based_result(
                current_token,
                plan_name.as_deref(),
                "Cursor request-based usage data unavailable. Try again later.",
            ) {
                return Ok(snapshot(mapped, UsageHistory::default(), Vec::new(), now));
            }
        }

        let credits = self
            .client
            .fetch_credits(current_token)
            .ok()
            .filter(|response| response.status.is_success())
            .and_then(|response| response.json());
        let stripe = self
            .client
            .fetch_stripe_balance(current_token)
            .ok()
            .flatten()
            .filter(|response| response.status.is_success())
            .and_then(|response| response.json());
        let mapped = map_live_usage(
            &usage,
            plan_name.as_deref(),
            credits.as_ref(),
            stripe_balance_cents(stripe.as_ref()),
        )?;
        let history = self.fetch_usage_history(current_token, now);
        Ok(snapshot(mapped, history, Vec::new(), now))
    }

    fn fetch_usage_with_retry(
        &self,
        access_token: &str,
        auth: &mut CursorAuthState,
    ) -> Result<CursorResponse, CursorError> {
        let first = self.client.fetch_usage(access_token)?;
        if !matches!(
            first.status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Ok(first);
        }
        let refreshed = self
            .refresh_access_token(auth)?
            .ok_or(CursorError::TokenExpired)?;
        self.client
            .fetch_usage(&refreshed)
            .map_err(|_| CursorError::UsageAfterRefreshFailed)
    }

    fn refresh_access_token(
        &self,
        auth: &mut CursorAuthState,
    ) -> Result<Option<String>, CursorError> {
        let Some(refresh_token) = auth
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
        else {
            return Ok(None);
        };
        let response = self.client.refresh_token(refresh_token)?;
        let body = response.json();
        if matches!(
            response.status,
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED
        ) {
            return Err(if should_logout(body.as_ref()) {
                CursorError::SessionExpired
            } else {
                CursorError::TokenExpired
            });
        }
        if !response.status.is_success() {
            return Ok(None);
        }
        let Some(body) = body else {
            return Ok(None);
        };
        if should_logout(Some(&body)) {
            return Err(CursorError::SessionExpired);
        }
        let access_token = body
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_owned);
        if let Some(access_token) = access_token {
            if auth.save_access_token(access_token.clone()).is_err() {
                auth.access_token = Some(access_token.clone());
                eprintln!(
                    "Cursor authentication: failed to persist rotated access token; using it for this session only"
                );
            }
            return Ok(Some(access_token));
        }
        Ok(None)
    }

    fn fetch_plan_name(&self, access_token: &str) -> (Option<String>, bool) {
        let Some(body) = self
            .client
            .fetch_plan(access_token)
            .ok()
            .filter(|response| response.status.is_success())
            .and_then(|response| response.json())
        else {
            return (None, true);
        };
        let name = body
            .pointer("/planInfo/planName")
            .and_then(Value::as_str)
            .map(str::to_owned);
        (name, false)
    }

    fn request_based_result(
        &self,
        access_token: &str,
        plan_name: Option<&str>,
        unavailable_message: &str,
    ) -> Result<mapper::CursorMappedUsage, CursorError> {
        let response = self
            .client
            .fetch_request_usage(access_token)
            .map_err(|_| CursorError::RequestBasedUnavailable(unavailable_message.into()))?
            .filter(|response| response.status.is_success())
            .ok_or_else(|| CursorError::RequestBasedUnavailable(unavailable_message.into()))?;
        let body = response
            .json()
            .ok_or_else(|| CursorError::RequestBasedUnavailable(unavailable_message.into()))?;
        map_request_usage(&body, plan_name, unavailable_message)
    }

    fn fetch_usage_history(&self, access_token: &str, now: chrono::DateTime<Utc>) -> UsageHistory {
        let local_now = now.with_timezone(&Local);
        let start_date = local_now.date_naive().checked_sub_days(Days::new(29));
        let Some(start) = start_date
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .and_then(|date| Local.from_local_datetime(&date).earliest())
        else {
            return UsageHistory::default();
        };
        let Some(response) = self
            .client
            .fetch_usage_csv(
                access_token,
                start.timestamp_millis(),
                now.timestamp_millis(),
            )
            .ok()
            .flatten()
            .filter(|response| response.status.is_success())
        else {
            return UsageHistory::default();
        };
        let Ok(csv) = std::str::from_utf8(&response.body) else {
            return UsageHistory::default();
        };
        let pricing = self.pricing.current();
        let rows = parse_usage_csv(csv, &pricing);
        usage_history(&rows, now, &pricing)
    }
}

fn snapshot(
    mapped: mapper::CursorMappedUsage,
    usage: UsageHistory,
    warnings: Vec<String>,
    refreshed_at: chrono::DateTime<Utc>,
) -> ProviderSnapshot {
    ProviderSnapshot {
        provider_id: "cursor".into(),
        plan: mapped.plan,
        quotas: mapped.quotas,
        value_metrics: mapped.value_metrics,
        notices: Vec::new(),
        usage,
        warnings,
        refreshed_at,
    }
}

fn require_success(response: &CursorResponse) -> Result<(), CursorError> {
    if response.status.is_success() {
        Ok(())
    } else if matches!(
        response.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        Err(CursorError::TokenExpired)
    } else {
        Err(CursorError::RequestFailed(response.status.as_u16()))
    }
}

fn json_object(response: &CursorResponse) -> Result<Value, CursorError> {
    response
        .json()
        .filter(Value::is_object)
        .ok_or(CursorError::InvalidResponse)
}

fn should_logout(value: Option<&Value>) -> bool {
    value
        .and_then(|value| value.get("shouldLogout"))
        .and_then(Value::as_bool)
        == Some(true)
}

impl crate::providers::UsageProvider for CursorProvider {
    fn definition(&self) -> ProviderDefinition {
        definition()
    }

    fn has_local_credentials(&self) -> bool {
        CursorAuthState::has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, crate::providers::ProviderError> {
        CursorProvider::refresh(self).map_err(|error| {
            use crate::models::ProviderErrorKind as Kind;
            let kind = match error {
                CursorError::NotLoggedIn
                | CursorError::SessionExpired
                | CursorError::TokenExpired => Kind::Authentication,
                CursorError::AuthWrite => Kind::CredentialStorage,
                CursorError::RequestFailed(429) => Kind::RateLimited,
                CursorError::ConnectionFailed
                | CursorError::RequestFailed(_)
                | CursorError::UsageAfterRefreshFailed
                | CursorError::RequestBasedUnavailable(_) => Kind::Network,
                CursorError::InvalidResponse
                | CursorError::TotalUsageLimitMissing
                | CursorError::NoActiveSubscription => Kind::InvalidResponse,
            };
            crate::providers::ProviderError::from_display(kind, error)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{TimeZone, Utc};
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{
        auth::{CursorAuthSource, CursorAuthState},
        client::{CursorClient, Endpoints},
        definition, CursorProvider,
    };
    use crate::pricing::PricingStore;

    fn jwt(subject: &str) -> String {
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::json!({"sub": subject, "exp": 9_999_999_999_i64}).to_string());
        format!("a.{payload}.c")
    }

    fn routing_server(refreshed_token: String) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        thread::spawn(move || {
            let mut usage_calls = 0;
            for _ in 0..7 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or_default();
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_owned();
                recorded.lock().unwrap().push(path.clone());
                let (status, content_type, body) = if path.starts_with("/usage") {
                    usage_calls += 1;
                    if usage_calls == 1 {
                        (401, "application/json", "{}".to_owned())
                    } else {
                        (
                            200,
                            "application/json",
                            r#"{"enabled":true,"billingCycleStart":1782864000000,"billingCycleEnd":1785542400000,"planUsage":{"limit":40000,"remaining":32000,"totalPercentUsed":20,"autoPercentUsed":10,"apiPercentUsed":5}}"#.to_owned(),
                        )
                    }
                } else if path.starts_with("/token") {
                    (
                        200,
                        "application/json",
                        serde_json::json!({"access_token": refreshed_token}).to_string(),
                    )
                } else if path.starts_with("/plan") {
                    (
                        200,
                        "application/json",
                        r#"{"planInfo":{"planName":"pro plan"}}"#.to_owned(),
                    )
                } else if path.starts_with("/credits") {
                    (
                        200,
                        "application/json",
                        r#"{"hasCreditGrants":false}"#.to_owned(),
                    )
                } else if path.starts_with("/stripe") {
                    (
                        200,
                        "application/json",
                        r#"{"customerBalance":-1000}"#.to_owned(),
                    )
                } else {
                    (
                        200,
                        "text/csv",
                        "Date,Model,Input (w/o Cache Write),Output Tokens\n2026-07-15T12:00:00Z,composer-1,100,0".to_owned(),
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (base, requests)
    }

    #[test]
    fn definition_matches_cursor_layout_contract() {
        let definition = definition();
        assert!(definition.fallback_enabled);
        assert_eq!(
            definition
                .metrics
                .iter()
                .map(|metric| metric.id.as_str())
                .collect::<Vec<_>>(),
            [
                "cursor.usage",
                "cursor.auto",
                "cursor.api",
                "cursor.onDemand",
                "cursor.requests",
                "cursor.credits",
                "cursor.trend",
                "cursor.today",
                "cursor.yesterday",
                "cursor.last30",
            ]
        );
        assert!(definition.metrics[1].default_pinned);
        assert!(definition.metrics[2].default_pinned);
        assert!(!definition.metrics[4].default_enabled);
        assert!(!definition.metrics[5].default_enabled);
    }

    #[test]
    fn provider_retries_auth_and_keeps_optional_csv_spend_additive() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.vscdb");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
                [],
            )
            .unwrap();
        drop(connection);
        let refreshed = jwt("google-oauth2|user_abc123");
        let (base, requests) = routing_server(refreshed.clone());
        let endpoints = Endpoints {
            usage: format!("{base}/usage"),
            plan: format!("{base}/plan"),
            credits: format!("{base}/credits"),
            refresh: format!("{base}/token"),
            rest_usage: format!("{base}/rest"),
            stripe: format!("{base}/stripe"),
            csv: format!("{base}/csv"),
        };
        let pricing = Arc::new(PricingStore::new(directory.path().join("pricing")).unwrap());
        let provider = CursorProvider {
            pricing,
            client: CursorClient::with_endpoints(endpoints).unwrap(),
        };
        let auth = CursorAuthState {
            access_token: Some(jwt("google-oauth2|old_user")),
            refresh_token: Some("refresh-token".into()),
            source: CursorAuthSource::Sqlite(database.clone()),
        };
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();

        let snapshot = provider.refresh_with_auth(auth, now).unwrap();

        assert_eq!(snapshot.plan.as_deref(), Some("Pro Plan"));
        assert_eq!(snapshot.quotas.len(), 3);
        assert_eq!(snapshot.value_metrics[0].id, "credits");
        assert_eq!(snapshot.usage.today.as_ref().unwrap().tokens, 100);
        assert!(snapshot.warnings.is_empty());
        let saved: String = Connection::open(database)
            .unwrap()
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, refreshed);
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|path| path.starts_with("/usage"))
                .count(),
            2
        );
        assert!(requests.iter().any(|path| path.starts_with("/csv?")));
    }

    #[test]
    fn provider_keeps_a_refreshed_token_in_memory_without_a_visible_save_warning() {
        let directory = tempdir().unwrap();
        let refreshed = jwt("google-oauth2|user_abc123");
        let (base, _) = routing_server(refreshed);
        let endpoints = Endpoints {
            usage: format!("{base}/usage"),
            plan: format!("{base}/plan"),
            credits: format!("{base}/credits"),
            refresh: format!("{base}/token"),
            rest_usage: format!("{base}/rest"),
            stripe: format!("{base}/stripe"),
            csv: format!("{base}/csv"),
        };
        let pricing = Arc::new(PricingStore::new(directory.path().join("pricing")).unwrap());
        let provider = CursorProvider {
            pricing,
            client: CursorClient::with_endpoints(endpoints).unwrap(),
        };
        let auth = CursorAuthState {
            access_token: Some(jwt("google-oauth2|old_user")),
            refresh_token: Some("refresh-token".into()),
            // A directory cannot be opened as a SQLite database, forcing persistence to fail.
            source: CursorAuthSource::Sqlite(directory.path().to_path_buf()),
        };
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 12, 0, 0).unwrap();

        let snapshot = provider.refresh_with_auth(auth, now).unwrap();

        assert_eq!(snapshot.plan.as_deref(), Some("Pro Plan"));
        assert!(snapshot.warnings.is_empty());
    }
}
