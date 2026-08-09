use crate::{
    models::ApiKeyStatus,
    providers::api_key::{ApiKeyStore, SecretString},
};

use super::MiniMaxError;

const CONFIG_PATHS: &[&str] = &["~/.config/openquota/minimax.json"];
const ENVIRONMENT_NAMES: &[&str] = &["MINIMAX_API_KEY"];

#[derive(Clone)]
pub struct MiniMaxAuthStore {
    store: ApiKeyStore,
}

impl MiniMaxAuthStore {
    pub fn new() -> Self {
        Self {
            store: ApiKeyStore::new_with_sources("minimax", ENVIRONMENT_NAMES, CONFIG_PATHS),
        }
    }

    #[cfg(test)]
    pub(super) fn with_store(store: ApiKeyStore) -> Self {
        Self { store }
    }

    pub fn load(&self) -> Result<Option<SecretString>, MiniMaxError> {
        self.store
            .load()
            .map_err(|_| MiniMaxError::CredentialStorage)
    }

    pub fn has_local_credentials(&self) -> bool {
        self.load().is_ok_and(|secret| secret.is_some())
    }

    pub fn status(&self) -> Result<ApiKeyStatus, MiniMaxError> {
        self.store
            .status()
            .map_err(|_| MiniMaxError::CredentialStorage)
    }

    pub fn save(&self, value: &str) -> Result<(), MiniMaxError> {
        self.store.save(value).map_err(|_| {
            if value.trim().is_empty() {
                MiniMaxError::MissingKey
            } else {
                crate::app_warn!("auth:minimax", "system credential store write failed");
                MiniMaxError::CredentialStorage
            }
        })
    }

    pub fn delete(&self) -> Result<(), MiniMaxError> {
        self.store.delete().map_err(|_| {
            crate::app_warn!("auth:minimax", "system credential store delete failed");
            MiniMaxError::CredentialStorage
        })
    }
}

impl Default for MiniMaxAuthStore {
    fn default() -> Self {
        Self::new()
    }
}
