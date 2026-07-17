use std::sync::Arc;

use crate::models::ApiKeyStatus;
use zeroize::Zeroizing;

use super::credential_store::{delete_owned_password, read_owned_password, write_owned_password};

const SERVICE: &str = "io.github.deviffyy.openquota.api-key";

pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    pub fn new(value: Vec<u8>) -> Self {
        Self(Zeroizing::new(value))
    }

    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub struct SecretString(Zeroizing<String>);

impl SecretString {
    fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

pub trait SecretBackend: Send + Sync {
    fn read(&self, account: &str) -> Result<Option<SecretBytes>, String>;
    fn write(&self, account: &str, value: &[u8]) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

#[derive(Default)]
struct SystemSecretBackend;

impl SecretBackend for SystemSecretBackend {
    fn read(&self, account: &str) -> Result<Option<SecretBytes>, String> {
        read_owned_password(SERVICE, account).map(|value| value.map(SecretBytes::new))
    }

    fn write(&self, account: &str, value: &[u8]) -> Result<(), String> {
        write_owned_password(SERVICE, account, value)
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        delete_owned_password(SERVICE, account)
    }
}

pub trait EnvironmentReader: Send + Sync {
    fn value(&self, name: &str) -> Option<String>;
}

#[derive(Default)]
struct ProcessEnvironment;

impl EnvironmentReader for ProcessEnvironment {
    fn value(&self, name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
}

#[derive(Clone)]
pub struct ApiKeyStore {
    provider_id: String,
    environment_name: String,
    secrets: Arc<dyn SecretBackend>,
    environment: Arc<dyn EnvironmentReader>,
}

impl ApiKeyStore {
    pub fn new(provider_id: &str, environment_name: &str) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            environment_name: environment_name.to_owned(),
            secrets: Arc::new(SystemSecretBackend),
            environment: Arc::new(ProcessEnvironment),
        }
    }

    #[cfg(test)]
    pub fn with_backends(
        provider_id: &str,
        environment_name: &str,
        secrets: Arc<dyn SecretBackend>,
        environment: Arc<dyn EnvironmentReader>,
    ) -> Self {
        Self {
            provider_id: provider_id.to_owned(),
            environment_name: environment_name.to_owned(),
            secrets,
            environment,
        }
    }

    pub fn load(&self) -> Result<Option<SecretString>, String> {
        match self.saved_key() {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => Ok(self.environment_key()),
            Err(error) => match self.environment_key() {
                Some(value) => {
                    report_environment_fallback(&self.provider_id, &error);
                    Ok(Some(value))
                }
                None => Err(error),
            },
        }
    }

    pub fn status(&self) -> Result<ApiKeyStatus, String> {
        let environment = self.environment_key().is_some();
        let saved = match self.saved_key() {
            Ok(value) => value.is_some(),
            Err(error) if environment => {
                report_environment_fallback(&self.provider_id, &error);
                return Ok(ApiKeyStatus::FromEnvironment);
            }
            Err(error) => return Err(error),
        };
        Ok(match (saved, environment) {
            (false, false) => ApiKeyStatus::NotSet,
            (false, true) => ApiKeyStatus::FromEnvironment,
            (true, false) => ApiKeyStatus::Saved,
            (true, true) => ApiKeyStatus::OverrideActive,
        })
    }

    pub fn save(&self, value: &str) -> Result<(), String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("Enter an API key before saving.".into());
        }
        self.secrets.write(&self.provider_id, value.as_bytes())
    }

    pub fn delete(&self) -> Result<(), String> {
        self.secrets.delete(&self.provider_id)
    }

    fn saved_key(&self) -> Result<Option<SecretString>, String> {
        let Some(value) = self.secrets.read(&self.provider_id)? else {
            return Ok(None);
        };
        let value = std::str::from_utf8(value.as_slice())
            .map_err(|_| "The saved API key has an unsupported encoding.".to_owned())?;
        Ok(non_empty(value.to_owned()))
    }

    fn environment_key(&self) -> Option<SecretString> {
        self.environment
            .value(&self.environment_name)
            .and_then(non_empty)
    }
}

fn report_environment_fallback(provider_id: &str, error: &str) {
    crate::app_warn!(
        &format!("auth:{provider_id}"),
        "system credential store unavailable; using environment API key ({error})"
    );
}

fn non_empty(value: String) -> Option<SecretString> {
    let value = SecretString::new(value);
    let trimmed = value.as_str().trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == value.len() {
        Some(value)
    } else {
        Some(SecretString::new(trimmed.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use crate::models::ApiKeyStatus;

    use super::{ApiKeyStore, EnvironmentReader, SecretBackend, SecretBytes};

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

    struct ReadErrorSecrets;

    impl SecretBackend for ReadErrorSecrets {
        fn read(&self, _account: &str) -> Result<Option<SecretBytes>, String> {
            Err("System credential store unavailable.".into())
        }

        fn write(&self, _account: &str, _value: &[u8]) -> Result<(), String> {
            Err("System credential store unavailable.".into())
        }

        fn delete(&self, _account: &str) -> Result<(), String> {
            Err("System credential store unavailable.".into())
        }
    }

    fn store(secrets: Arc<MemorySecrets>, environment: &[(&str, &str)]) -> ApiKeyStore {
        ApiKeyStore::with_backends(
            "provider",
            "PROVIDER_API_KEY",
            secrets,
            Arc::new(MemoryEnvironment(
                environment
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            )),
        )
    }

    #[test]
    fn saved_key_overrides_environment_and_delete_falls_back() {
        let secrets = Arc::new(MemorySecrets::default());
        let store = store(secrets, &[("PROVIDER_API_KEY", " environment-key ")]);
        assert_eq!(store.status().unwrap(), ApiKeyStatus::FromEnvironment);
        assert_eq!(
            store.load().unwrap().as_ref().map(|value| value.as_str()),
            Some("environment-key")
        );

        store.save(" saved-key ").unwrap();
        assert_eq!(store.status().unwrap(), ApiKeyStatus::OverrideActive);
        assert_eq!(
            store.load().unwrap().as_ref().map(|value| value.as_str()),
            Some("saved-key")
        );

        store.delete().unwrap();
        assert_eq!(store.status().unwrap(), ApiKeyStatus::FromEnvironment);
        assert_eq!(
            store.load().unwrap().as_ref().map(|value| value.as_str()),
            Some("environment-key")
        );
    }

    #[test]
    fn saved_only_and_empty_states_are_reported_without_exposing_the_key() {
        let secrets = Arc::new(MemorySecrets::default());
        let store = store(secrets, &[]);
        assert_eq!(store.status().unwrap(), ApiKeyStatus::NotSet);
        assert!(store.save("  ").is_err());

        store.save("secret").unwrap();
        assert_eq!(store.status().unwrap(), ApiKeyStatus::Saved);
    }

    #[test]
    fn environment_key_remains_available_when_the_system_store_cannot_be_read() {
        let store = ApiKeyStore::with_backends(
            "provider",
            "PROVIDER_API_KEY",
            Arc::new(ReadErrorSecrets),
            Arc::new(MemoryEnvironment(HashMap::from([(
                "PROVIDER_API_KEY".into(),
                " environment-key ".into(),
            )]))),
        );

        assert_eq!(store.status().unwrap(), ApiKeyStatus::FromEnvironment);
        assert_eq!(
            store.load().unwrap().as_ref().map(|value| value.as_str()),
            Some("environment-key")
        );
    }

    #[test]
    fn credential_store_errors_are_not_hidden_without_an_environment_key() {
        let store = ApiKeyStore::with_backends(
            "provider",
            "PROVIDER_API_KEY",
            Arc::new(ReadErrorSecrets),
            Arc::new(MemoryEnvironment(HashMap::new())),
        );

        assert_eq!(
            store.status().unwrap_err(),
            "System credential store unavailable."
        );
        assert_eq!(
            store.load().err().as_deref(),
            Some("System credential store unavailable.")
        );
    }
}
