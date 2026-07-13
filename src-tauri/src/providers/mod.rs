pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod credential_store;
#[cfg(test)]
pub mod test_http;

use crate::models::ProviderSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Authentication,
    Permission,
    RateLimited,
    Network,
    InvalidResponse,
    CredentialStorage,
    LocalData,
    Storage,
    Internal,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    _kind: ProviderErrorKind,
    message: String,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            _kind: kind,
            message: message.into(),
        }
    }

    pub fn from_display(kind: ProviderErrorKind, error: impl std::fmt::Display) -> Self {
        Self::new(kind, error.to_string())
    }

    #[cfg(test)]
    fn kind(&self) -> ProviderErrorKind {
        self._kind
    }
}

pub trait UsageProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn has_local_credentials(&self) -> bool;
    fn refresh(&self) -> Result<ProviderSnapshot, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::{ProviderError, ProviderErrorKind};

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
}
