use std::{env, fs, path::PathBuf};

use serde::Deserialize;

use super::CommandCodeError;

#[derive(Debug, Deserialize)]
struct AuthDocument {
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
}

#[derive(Clone)]
pub struct CommandCodeAuthStore {
    path: PathBuf,
}

impl CommandCodeAuthStore {
    pub fn new() -> Self {
        Self {
            path: home_directory().join(".commandcode").join("auth.json"),
        }
    }

    pub fn load(&self) -> Result<String, CommandCodeError> {
        let text = fs::read_to_string(&self.path).map_err(|_| CommandCodeError::NotLoggedIn)?;
        let document: AuthDocument =
            serde_json::from_str(&text).map_err(|_| CommandCodeError::InvalidAuth)?;
        document
            .api_key
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())
            .ok_or(CommandCodeError::InvalidAuth)
    }

    pub fn has_local_credentials(&self) -> bool {
        self.load().is_ok()
    }
}

impl Default for CommandCodeAuthStore {
    fn default() -> Self {
        Self::new()
    }
}

fn home_directory() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
