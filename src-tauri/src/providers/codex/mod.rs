pub mod auth;
pub mod client;
pub mod local_usage;
pub mod mapper;
pub mod pricing;

use std::sync::Arc;

use chrono::Utc;
use reqwest::StatusCode;
use thiserror::Error;

use crate::{models::ProviderSnapshot, storage::Storage};

use self::{
    auth::CodexAuthState, client::CodexClient, local_usage::scan_local_usage, mapper::map_usage,
};

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("Not logged in. Run `codex` to authenticate.")]
    NotLoggedIn,
    #[error(
        "Subscription usage is unavailable for API-key-only logins. Sign in to Codex with ChatGPT."
    )]
    ApiKeyOnly,
    #[error("Your Codex session expired. Run `codex` to sign in again.")]
    SessionExpired,
    #[error("Codex credentials changed while refreshing. Run `codex` to sign in again.")]
    TokenConflict,
    #[error("Your Codex session was revoked. Run `codex` to sign in again.")]
    TokenRevoked,
    #[error("Your Codex access token expired. Run `codex` to sign in again.")]
    TokenExpired,
    #[error("Codex auth data is invalid. Run `codex` to sign in again.")]
    InvalidAuth,
    #[error("Refreshed Codex credentials could not be saved.")]
    AuthWrite,
    #[error("Codex usage request failed (HTTP {0}).")]
    RequestFailed(u16),
    #[error("Codex returned an invalid usage response.")]
    InvalidResponse,
    #[error("Could not connect to Codex. Check your internet connection.")]
    ConnectionFailed,
    #[error("Local Codex usage logs could not be processed.")]
    LocalUsage,
    #[error("OpenQuota cache is unavailable.")]
    Storage,
}

impl From<crate::storage::StorageError> for CodexError {
    fn from(_: crate::storage::StorageError) -> Self {
        Self::Storage
    }
}

pub struct CodexProvider {
    storage: Arc<Storage>,
    client: CodexClient,
}

impl CodexProvider {
    pub fn new(storage: Arc<Storage>) -> Result<Self, CodexError> {
        Ok(Self {
            storage,
            client: CodexClient::new()?,
        })
    }

    pub fn refresh(&self) -> Result<ProviderSnapshot, CodexError> {
        let now = Utc::now();
        let mut auth = CodexAuthState::load()?;
        let mut warnings = Vec::new();

        if auth.needs_refresh(now) {
            if let Ok(live) = auth.reload() {
                auth = live;
            }
        }
        if auth.needs_refresh(now) {
            self.refresh_access_token(&mut auth, now, &mut warnings)?;
        }

        let mut response = self
            .client
            .fetch_usage(&auth.access_token, auth.account_id.as_deref())?;
        if matches!(
            response.status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.refresh_access_token(&mut auth, now, &mut warnings)?;
            response = self
                .client
                .fetch_usage(&auth.access_token, auth.account_id.as_deref())?;
        }
        let mapped = map_usage(&response, now)?;
        let usage = scan_local_usage(&self.storage, now)?;
        Ok(ProviderSnapshot {
            provider_id: "codex".into(),
            plan: mapped.plan,
            quotas: mapped.quotas,
            usage,
            warnings,
            refreshed_at: now,
        })
    }

    fn refresh_access_token(
        &self,
        auth: &mut CodexAuthState,
        now: chrono::DateTime<Utc>,
        warnings: &mut Vec<String>,
    ) -> Result<(), CodexError> {
        let refresh_token = auth
            .refresh_token
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(CodexError::TokenExpired)?;
        let refreshed = self.client.refresh_token(refresh_token)?;
        if auth
            .update_and_save(
                refreshed.access_token,
                refreshed.refresh_token,
                refreshed.id_token,
                now,
            )
            .is_err()
        {
            warnings.push(
                "The refreshed Codex login is active for this session but could not be saved."
                    .into(),
            );
        }
        Ok(())
    }
}

impl crate::providers::UsageProvider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn has_local_credentials(&self) -> bool {
        CodexAuthState::has_local_credentials()
    }

    fn refresh(&self) -> Result<ProviderSnapshot, String> {
        CodexProvider::refresh(self).map_err(|error| error.to_string())
    }
}
