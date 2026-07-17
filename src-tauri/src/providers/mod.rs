pub mod antigravity;
pub mod api_key;
pub mod claude;
pub mod codex;
pub mod credential_store;
pub mod cursor;
mod daily_usage;
mod detection;
mod log_usage;
pub mod openrouter;
mod registry;
#[cfg(test)]
pub mod test_http;

pub use detection::detect_local_credentials;
pub use registry::ProviderRegistry;

use crate::models::{ApiKeyStatus, ProviderDefinition, ProviderErrorKind, ProviderSnapshot};

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
    use super::{antigravity, claude, codex, cursor, openrouter, ProviderError};
    use crate::models::ProviderErrorKind;

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
            links(openrouter::definition()),
            [
                ("Activity".into(), "https://openrouter.ai/activity".into()),
                (
                    "Credits".into(),
                    "https://openrouter.ai/settings/credits".into()
                ),
            ]
        );
    }
}
