pub mod antigravity;
pub mod api_key;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod credential_store;
pub mod cursor;
mod daily_usage;
mod detection;
pub mod devin;
pub mod grok;
pub mod kimi;
mod log_usage;
pub mod minimax;
pub mod opencode;
pub mod openrouter;
mod pi_usage;
mod registry;
#[cfg(test)]
pub mod test_http;
pub mod zai;

pub use detection::{detect_local_credentials, CredentialProbeResults, CredentialProbeStatus};
pub use registry::ProviderRegistry;

use crate::models::{ApiKeyStatus, ProviderDefinition, ProviderErrorKind, ProviderSnapshot};

pub fn provider_family(provider_id: &str) -> &str {
    provider_id
        .split_once('@')
        .map(|(family, _)| family)
        .unwrap_or(provider_id)
}

pub fn is_account_provider_id(provider_id: &str, family: &str) -> bool {
    provider_id
        .strip_prefix(family)
        .and_then(|rest| rest.strip_prefix('@'))
        .is_some_and(|suffix| {
            suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

pub fn is_claude_account_provider_id(provider_id: &str) -> bool {
    is_account_provider_id(provider_id, "claude")
}

pub fn is_codex_account_provider_id(provider_id: &str) -> bool {
    is_account_provider_id(provider_id, "codex")
}

pub fn remember_default_account(
    storage: &crate::storage::Storage,
    family: &str,
    identity: &str,
) -> Result<(), crate::storage::StorageError> {
    let records = storage.load_provider_account_records(family)?;
    if records
        .iter()
        .any(|(known_identity, provider_id, _)| known_identity == identity && provider_id == family)
    {
        return Ok(());
    }
    if records
        .iter()
        .any(|(known_identity, provider_id, _)| known_identity == identity || provider_id == family)
    {
        return Ok(());
    }
    storage.save_provider_account_record(family, identity, family, "{}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheIdentity<'a> {
    Unscoped,
    Resolved(&'a str),
    Unresolved,
}

pub struct AccountRefresh {
    pub family: &'static str,
    pub provider_id: &'static str,
    pub identity: String,
}

pub struct ProviderRefresh {
    pub snapshot: ProviderSnapshot,
    pub cache_identity: Option<String>,
    pub account: Option<AccountRefresh>,
}

impl<'a> CacheIdentity<'a> {
    pub fn resolved_value(self) -> Option<&'a str> {
        match self {
            Self::Resolved(value) => Some(value),
            Self::Unscoped | Self::Unresolved => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    kind: ProviderErrorKind,
    message: String,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn from_display(kind: ProviderErrorKind, error: impl std::fmt::Display) -> Self {
        Self::new(kind, error.to_string())
    }

    pub fn kind(&self) -> ProviderErrorKind {
        self.kind
    }
}

pub trait UsageProvider: Send + Sync {
    fn definition(&self) -> ProviderDefinition;
    fn has_local_credentials(&self) -> bool;
    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError>;

    fn refresh_for_service(&self) -> Result<ProviderRefresh, ProviderError> {
        let snapshot = self.refresh()?;
        Ok(ProviderRefresh {
            snapshot,
            cache_identity: self.cache_identity().resolved_value().map(str::to_owned),
            account: None,
        })
    }

    fn cache_identity(&self) -> CacheIdentity<'_> {
        CacheIdentity::Unscoped
    }

    fn supports_account_names(&self) -> bool {
        false
    }

    fn supports_api_key_configuration(&self) -> bool {
        false
    }

    fn account_identity(&self) -> Option<&str> {
        None
    }

    fn api_key_status(&self) -> Option<Result<ApiKeyStatus, ProviderError>> {
        None
    }

    fn save_api_key(&self, _value: &str) -> Result<(), ProviderError> {
        Err(ProviderError::new(
            ProviderErrorKind::Internal,
            "That provider does not accept an API key.",
        ))
    }

    fn delete_api_key(&self) -> Result<(), ProviderError> {
        Err(ProviderError::new(
            ProviderErrorKind::Internal,
            "That provider does not accept an API key.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        antigravity, claude, codex, copilot, cursor, devin, grok, kimi, minimax, opencode,
        openrouter, remember_default_account, zai, ProviderError,
    };
    use crate::models::ProviderErrorKind;
    use tempfile::tempdir;

    #[test]
    fn remembered_default_account_is_stable_across_identity_changes() {
        let directory = tempdir().unwrap();
        let storage =
            crate::storage::Storage::open(&directory.path().join("openquota.db")).unwrap();

        remember_default_account(&storage, "codex", "identity-a").unwrap();
        remember_default_account(&storage, "codex", "identity-b").unwrap();

        assert_eq!(
            storage.load_provider_account_records("codex").unwrap(),
            [("identity-a".into(), "codex".into(), "{}".into())]
        );
    }

    #[test]
    fn provider_errors_expose_only_the_safe_message() {
        let error = ProviderError::new(
            ProviderErrorKind::Network,
            "Could not connect to the provider.",
        );

        assert_eq!(error.kind(), ProviderErrorKind::Network);
        assert_eq!(error.to_string(), "Could not connect to the provider.");
        assert!(!error.to_string().contains("secret-token"));
    }

    #[test]
    fn provider_quick_links_match_the_declared_browser_destinations() {
        let links = |definition: crate::models::ProviderDefinition| {
            definition
                .links
                .into_iter()
                .map(|link| (link.label, link.url))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            links(claude::definition()),
            [
                ("Status".into(), "https://status.anthropic.com/".into()),
                (
                    "Dashboard".into(),
                    "https://claude.ai/settings/usage".into()
                ),
            ]
        );
        assert_eq!(
            links(codex::definition()),
            [
                ("Status".into(), "https://status.openai.com/".into()),
                (
                    "Dashboard".into(),
                    "https://chatgpt.com/codex/settings/usage".into()
                ),
            ]
        );
        assert_eq!(
            links(cursor::definition()),
            [
                ("Status".into(), "https://status.cursor.com/".into()),
                (
                    "Dashboard".into(),
                    "https://www.cursor.com/dashboard".into()
                ),
            ]
        );
        assert!(links(antigravity::definition()).is_empty());
        assert_eq!(
            links(copilot::definition()),
            [
                ("Status".into(), "https://www.githubstatus.com/".into()),
                (
                    "Dashboard".into(),
                    "https://github.com/settings/billing".into()
                ),
            ]
        );
        assert_eq!(
            links(devin::definition()),
            [(
                "Dashboard".into(),
                "https://app.devin.ai/settings/plans".into()
            )]
        );
        assert_eq!(
            links(grok::definition()),
            [("Usage".into(), "https://grok.com/?_s=usage".into())]
        );
        assert_eq!(
            links(opencode::definition()),
            [("Dashboard".into(), "https://opencode.ai/auth".into())]
        );
        assert_eq!(
            links(openrouter::definition()),
            [
                ("Activity".into(), "https://openrouter.ai/activity".into()),
                (
                    "Credits".into(),
                    "https://openrouter.ai/settings/credits".into()
                ),
            ]
        );
        assert_eq!(
            links(zai::definition()),
            [
                (
                    "Dashboard".into(),
                    "https://z.ai/manage-apikey/coding-plan/personal/my-plan".into()
                ),
                (
                    "API Keys".into(),
                    "https://z.ai/manage-apikey/apikey-list".into()
                ),
            ]
        );
        assert_eq!(
            links(kimi::definition()),
            [
                (
                    "Dashboard".into(),
                    "https://www.kimi.com/code/console".into()
                ),
                (
                    "API Keys".into(),
                    "https://www.kimi.com/code/console".into()
                ),
            ]
        );
        assert_eq!(
            links(minimax::definition()),
            [
                (
                    "Dashboard".into(),
                    "https://platform.minimax.io/console/plan".into()
                ),
                (
                    "API Keys".into(),
                    "https://platform.minimax.io/console/access".into()
                ),
            ]
        );
    }
}
