use crate::{
    models::ApiKeyStatus,
    providers::api_key::{ApiKeyStore, SecretString},
};

use super::ZaiError;

const CONFIG_PATHS: &[&str] = &["~/.config/openquota/zai.json", "~/.config/zai/key.json"];
const ENVIRONMENT_NAMES: &[&str] = &["ZAI_API_KEY", "GLM_API_KEY"];

#[derive(Clone)]
pub struct ZaiAuthStore {
    store: ApiKeyStore,
}

impl ZaiAuthStore {
    pub fn new() -> Self {
        Self {
            store: ApiKeyStore::new_with_sources("zai", ENVIRONMENT_NAMES, CONFIG_PATHS),
        }
    }

    #[cfg(test)]
    pub(super) fn with_store(store: ApiKeyStore) -> Self {
        Self { store }
    }

    pub fn load(&self) -> Result<Option<SecretString>, ZaiError> {
        self.store.load().map_err(|_| ZaiError::CredentialStorage)
    }

    pub fn has_local_credentials(&self) -> bool {
        self.load().is_ok_and(|secret| secret.is_some())
    }

    pub fn status(&self) -> Result<ApiKeyStatus, ZaiError> {
        self.store.status().map_err(|_| ZaiError::CredentialStorage)
    }

    pub fn save(&self, value: &str) -> Result<(), ZaiError> {
        self.store.save(value).map_err(|_| {
            if value.trim().is_empty() {
                ZaiError::MissingKey
            } else {
                crate::app_warn!("auth:zai", "system credential store write failed");
                ZaiError::CredentialStorage
            }
        })
    }

    pub fn delete(&self) -> Result<(), ZaiError> {
        self.store.delete().map_err(|_| {
            crate::app_warn!("auth:zai", "system credential store delete failed");
            ZaiError::CredentialStorage
        })
    }
}

impl Default for ZaiAuthStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use crate::{
        models::ApiKeyStatus,
        providers::api_key::{
            ApiKeyStore, ConfigFileReader, EnvironmentReader, SecretBackend, SecretBytes,
        },
    };

    use super::{ZaiAuthStore, CONFIG_PATHS, ENVIRONMENT_NAMES};

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<String, Vec<u8>>>);

    impl SecretBackend for MemorySecrets {
        fn read(&self, account: &str) -> Result<Option<SecretBytes>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .map(SecretBytes::new))
        }

        fn write(&self, account: &str, value: &[u8]) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_owned(), value.to_vec());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    struct MemoryEnvironment(HashMap<String, String>);

    impl EnvironmentReader for MemoryEnvironment {
        fn value(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    struct MemoryConfigs(HashMap<String, Vec<u8>>);

    impl ConfigFileReader for MemoryConfigs {
        fn read(&self, path: &str) -> Option<SecretBytes> {
            self.0.get(path).cloned().map(SecretBytes::new)
        }
    }

    struct ReadErrorSecrets;

    impl SecretBackend for ReadErrorSecrets {
        fn read(&self, _account: &str) -> Result<Option<SecretBytes>, String> {
            Err("backend diagnostic contains secret-value".into())
        }

        fn write(&self, _account: &str, _value: &[u8]) -> Result<(), String> {
            Err("backend diagnostic contains secret-value".into())
        }

        fn delete(&self, _account: &str) -> Result<(), String> {
            Err("backend diagnostic contains secret-value".into())
        }
    }

    fn store(
        secrets: Arc<dyn SecretBackend>,
        environment: &[(&str, &str)],
        configs: &[(&str, &[u8])],
    ) -> ZaiAuthStore {
        ZaiAuthStore::with_store(ApiKeyStore::with_source_backends(
            "zai",
            ENVIRONMENT_NAMES,
            CONFIG_PATHS,
            secrets,
            Arc::new(MemoryEnvironment(
                environment
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
            )),
            Arc::new(MemoryConfigs(
                configs
                    .iter()
                    .map(|(path, value)| ((*path).to_owned(), value.to_vec()))
                    .collect(),
            )),
        ))
    }

    #[test]
    fn vault_overrides_config_then_environment_names_follow_declared_order() {
        let secrets = Arc::new(MemorySecrets::default());
        let auth = store(
            secrets,
            &[
                (ENVIRONMENT_NAMES[0], "primary-env"),
                (ENVIRONMENT_NAMES[1], "legacy-env"),
            ],
            &[
                (CONFIG_PATHS[0], br#"{"api_key":"primary-config"}"#),
                (CONFIG_PATHS[1], b"alternate-config"),
            ],
        );

        assert_eq!(auth.load().unwrap().unwrap().as_str(), "primary-config");
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::FromConfig);

        auth.save(" vault-key ").unwrap();
        assert_eq!(auth.load().unwrap().unwrap().as_str(), "vault-key");
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::OverrideActive);

        auth.delete().unwrap();
        assert_eq!(auth.load().unwrap().unwrap().as_str(), "primary-config");
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::FromConfig);

        let primary_env = store(
            Arc::new(MemorySecrets::default()),
            &[
                (ENVIRONMENT_NAMES[0], "primary-env"),
                (ENVIRONMENT_NAMES[1], "legacy-env"),
            ],
            &[],
        );
        assert_eq!(primary_env.load().unwrap().unwrap().as_str(), "primary-env");

        let legacy_env = store(
            Arc::new(MemorySecrets::default()),
            &[(ENVIRONMENT_NAMES[1], " legacy-env ")],
            &[],
        );
        assert_eq!(legacy_env.load().unwrap().unwrap().as_str(), "legacy-env");
    }

    #[test]
    fn alternate_and_plain_text_configs_are_supported_before_environment() {
        let auth = store(
            Arc::new(MemorySecrets::default()),
            &[(ENVIRONMENT_NAMES[0], "environment")],
            &[
                (CONFIG_PATHS[0], br#"{"apiKey":42}"#),
                (CONFIG_PATHS[1], b" alternate-config\n"),
            ],
        );

        assert_eq!(auth.load().unwrap().unwrap().as_str(), "alternate-config");
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::FromConfig);
    }

    #[test]
    fn vault_only_and_empty_states_are_reported_and_clear_cleanly() {
        let auth = store(Arc::new(MemorySecrets::default()), &[], &[]);
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::NotSet);
        assert!(!auth.has_local_credentials());
        assert!(auth.save("   ").is_err());

        auth.save("saved-key").unwrap();
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::Saved);
        assert!(auth.has_local_credentials());

        auth.delete().unwrap();
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::NotSet);
    }

    #[test]
    fn external_fallback_survives_vault_read_failures_without_leaking_diagnostics() {
        let auth = store(
            Arc::new(ReadErrorSecrets),
            &[(ENVIRONMENT_NAMES[0], "environment-key")],
            &[],
        );

        assert_eq!(auth.load().unwrap().unwrap().as_str(), "environment-key");
        assert_eq!(auth.status().unwrap(), ApiKeyStatus::FromEnvironment);

        let without_fallback = store(Arc::new(ReadErrorSecrets), &[], &[]);
        let error = without_fallback.load().err().unwrap();
        assert_eq!(
            error.to_string(),
            "The Z.ai API key could not be read or updated."
        );
        assert!(!error.to_string().contains("secret-value"));
    }
}
