use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{types::ValueRef, Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

pub const DEFAULT_API_SERVER_URL: &str = "https://server.codeium.com";

const AUTH_STATE_KEY: &str = "windsurfAuthStatus";
const MAX_CREDENTIAL_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct DevinAuth {
    pub api_key: String,
    pub api_server_url: Option<String>,
}

impl std::fmt::Debug for DevinAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DevinAuth")
            .field("api_key", &"<redacted>")
            .field(
                "api_server_url",
                &self.api_server_url.as_ref().map(|_| "<configured>"),
            )
            .finish()
    }
}

impl DevinAuth {
    pub fn effective_api_server_url(&self) -> &str {
        self.api_server_url
            .as_deref()
            .unwrap_or(DEFAULT_API_SERVER_URL)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostPlatform {
    #[cfg(any(target_os = "macos", test))]
    Macos,
    #[cfg(any(windows, test))]
    Windows,
    #[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
    Linux,
}

impl HostPlatform {
    fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(windows)]
        {
            Self::Windows
        }
        #[cfg(all(not(target_os = "macos"), not(windows)))]
        {
            Self::Linux
        }
    }
}

#[derive(Clone, Default)]
struct PlatformDirectories {
    home: Option<PathBuf>,
    #[cfg(any(not(windows), test))]
    xdg_data_home: Option<PathBuf>,
    #[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
    xdg_config_home: Option<PathBuf>,
    #[cfg(any(windows, test))]
    local_app_data: Option<PathBuf>,
    #[cfg(any(windows, test))]
    roaming_app_data: Option<PathBuf>,
}

impl PlatformDirectories {
    fn from_environment() -> Self {
        Self {
            home: environment_path("HOME").or_else(|| environment_path("USERPROFILE")),
            #[cfg(any(not(windows), test))]
            xdg_data_home: environment_path("XDG_DATA_HOME"),
            #[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
            xdg_config_home: environment_path("XDG_CONFIG_HOME"),
            #[cfg(any(windows, test))]
            local_app_data: environment_path("LOCALAPPDATA"),
            #[cfg(any(windows, test))]
            roaming_app_data: environment_path("APPDATA"),
        }
    }
}

#[derive(Clone)]
pub(super) struct DevinAuthStore {
    credential_paths: Vec<PathBuf>,
    state_db_paths: Vec<PathBuf>,
}

impl DevinAuthStore {
    pub fn new() -> Self {
        let directories = PlatformDirectories::from_environment();
        let credential_paths = environment_path("OPENQUOTA_DEVIN_CREDENTIALS_FILE")
            .map(|path| vec![path])
            .unwrap_or_else(|| credential_paths(HostPlatform::current(), &directories));
        let state_db_paths = environment_path("OPENQUOTA_DEVIN_STATE_DB")
            .map(|path| vec![path])
            .unwrap_or_else(|| state_db_paths(HostPlatform::current(), &directories));
        Self {
            credential_paths,
            state_db_paths,
        }
    }

    #[cfg(test)]
    pub fn with_paths(credential_paths: Vec<PathBuf>, state_db_paths: Vec<PathBuf>) -> Self {
        Self {
            credential_paths,
            state_db_paths,
        }
    }

    pub fn load_credentials_file(&self) -> Option<DevinAuth> {
        self.credential_paths
            .iter()
            .find_map(|path| load_credentials_file(path))
    }

    pub fn load_credentials_files(&self) -> Vec<DevinAuth> {
        self.credential_paths
            .iter()
            .filter_map(|path| load_credentials_file(path))
            .collect()
    }

    pub fn load_app_auth(&self) -> Option<DevinAuth> {
        self.state_db_paths
            .iter()
            .find_map(|path| load_app_auth(path))
    }

    pub fn load_app_auth_candidates(&self) -> Vec<DevinAuth> {
        self.state_db_paths
            .iter()
            .filter_map(|path| load_app_auth(path))
            .collect()
    }

    pub fn load_candidates(&self) -> Vec<DevinAuth> {
        deduplicate_candidates(
            self.load_credentials_files()
                .into_iter()
                .chain(self.load_app_auth_candidates()),
        )
    }

    pub fn has_local_credentials(&self) -> bool {
        self.load_credentials_file().is_some() || self.load_app_auth().is_some()
    }
}

pub(super) fn deduplicate_candidates(
    candidates: impl IntoIterator<Item = DevinAuth>,
) -> Vec<DevinAuth> {
    let mut unique = Vec::new();
    for candidate in candidates {
        let duplicate = unique.iter().any(|existing: &DevinAuth| {
            existing.api_key == candidate.api_key
                && existing.effective_api_server_url() == candidate.effective_api_server_url()
        });
        if !duplicate {
            unique.push(candidate);
        }
    }
    unique
}

fn load_credentials_file(path: &Path) -> Option<DevinAuth> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin CLI credential candidate could not be inspected"
            );
            return None;
        }
    };
    if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
        crate::app_warn!(
            "auth:devin",
            "a Devin CLI credential candidate was not a readable credential file"
        );
        return None;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin CLI credential candidate could not be read"
            );
            return None;
        }
    };
    let Some(api_key) = read_toml_string(&text, "windsurf_api_key").and_then(non_empty) else {
        crate::app_warn!("auth:devin", "Devin CLI credential data was malformed");
        return None;
    };
    Some(DevinAuth {
        api_key,
        api_server_url: clean_api_server_url(read_toml_string(&text, "api_server_url")),
    })
}

fn load_app_auth(path: &Path) -> Option<DevinAuth> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin app credential candidate was not a database file"
            );
            return None;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin app credential candidate could not be inspected"
            );
            return None;
        }
    }
    let connection = match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(connection) => connection,
        Err(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin app credential database could not be opened"
            );
            return None;
        }
    };
    let _ = connection.busy_timeout(Duration::from_millis(75));
    let value = match connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
            [AUTH_STATE_KEY],
            |row| match row.get_ref(0)? {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                    Ok(String::from_utf8(bytes.to_vec()).ok())
                }
                _ => Ok(None),
            },
        )
        .optional()
    {
        Ok(Some(Some(value))) => value,
        Ok(None | Some(None)) => return None,
        Err(_) => {
            crate::app_warn!(
                "auth:devin",
                "a Devin app credential database could not be queried"
            );
            return None;
        }
    };
    let body: Value = match serde_json::from_str(&value) {
        Ok(body) => body,
        Err(_) => {
            crate::app_warn!("auth:devin", "Devin app credential data was malformed");
            return None;
        }
    };
    let api_key = body
        .get("apiKey")
        .and_then(Value::as_str)
        .and_then(non_empty);
    let Some(api_key) = api_key else {
        crate::app_warn!("auth:devin", "Devin app credential data was unusable");
        return None;
    };
    Some(DevinAuth {
        api_key,
        api_server_url: None,
    })
}

fn clean_api_server_url(value: Option<String>) -> Option<String> {
    let value = non_empty(value?)?;
    if !value.starts_with("https://") {
        return None;
    }
    let value = value.trim_end_matches('/');
    (!value.is_empty()).then(|| value.to_owned())
}

fn read_toml_string(text: &str, key: &str) -> Option<String> {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((candidate, value)) = line.split_once('=') else {
            continue;
        };
        if candidate.trim() != key {
            continue;
        }
        return parse_toml_string_value(value.trim());
    }
    None
}

fn parse_toml_string_value(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    if value.starts_with('"') {
        return parse_basic_toml_string(value);
    }
    if let Some(rest) = value.strip_prefix('\'') {
        let closing = rest.find('\'')?;
        let parsed = non_empty(&rest[..closing])?;
        return valid_value_tail(&rest[closing + 1..]).then_some(parsed);
    }
    let value = value.split_once('#').map_or(value, |(value, _)| value);
    non_empty(value)
}

fn parse_basic_toml_string(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut escaped = false;
    let mut closing = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'"' => {
                closing = Some(index);
                break;
            }
            _ => {}
        }
    }
    let closing = closing?;
    if !valid_value_tail(&value[closing + 1..]) {
        return None;
    }
    let parsed: String = serde_json::from_str(&value[..=closing]).ok()?;
    non_empty(parsed)
}

fn valid_value_tail(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || value.starts_with('#')
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn credential_paths(platform: HostPlatform, directories: &PlatformDirectories) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    match platform {
        #[cfg(any(target_os = "macos", test))]
        HostPlatform::Macos => {
            push_join(
                &mut paths,
                directories.home.as_ref(),
                ".local/share/devin/credentials.toml",
            );
            push_join(
                &mut paths,
                directories.xdg_data_home.as_ref(),
                "devin/credentials.toml",
            );
        }
        #[cfg(any(windows, test))]
        HostPlatform::Windows => {
            push_join(
                &mut paths,
                directories.local_app_data.as_ref(),
                "devin/credentials.toml",
            );
            push_join(
                &mut paths,
                directories.home.as_ref(),
                ".local/share/devin/credentials.toml",
            );
        }
        #[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
        HostPlatform::Linux => {
            push_join(
                &mut paths,
                directories.xdg_data_home.as_ref(),
                "devin/credentials.toml",
            );
            push_join(
                &mut paths,
                directories.home.as_ref(),
                ".local/share/devin/credentials.toml",
            );
        }
    }
    unique_paths(paths)
}

fn state_db_paths(platform: HostPlatform, directories: &PlatformDirectories) -> Vec<PathBuf> {
    let suffix = "Devin/User/globalStorage/state.vscdb";
    let mut paths = Vec::new();
    match platform {
        #[cfg(any(target_os = "macos", test))]
        HostPlatform::Macos => push_join(
            &mut paths,
            directories.home.as_ref(),
            "Library/Application Support/Devin/User/globalStorage/state.vscdb",
        ),
        #[cfg(any(windows, test))]
        HostPlatform::Windows => {
            push_join(&mut paths, directories.roaming_app_data.as_ref(), suffix);
            push_join(&mut paths, directories.local_app_data.as_ref(), suffix);
        }
        #[cfg(any(all(not(target_os = "macos"), not(windows)), test))]
        HostPlatform::Linux => {
            push_join(&mut paths, directories.xdg_config_home.as_ref(), suffix);
            push_join(
                &mut paths,
                directories.home.as_ref(),
                ".config/Devin/User/globalStorage/state.vscdb",
            );
        }
    }
    unique_paths(paths)
}

fn push_join(paths: &mut Vec<PathBuf>, root: Option<&PathBuf>, suffix: &str) {
    if let Some(root) = root {
        paths.push(root.join(suffix));
    }
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn non_empty(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
