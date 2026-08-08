use std::{collections::HashMap, sync::OnceLock};

#[cfg(unix)]
use std::{sync::Arc, time::Duration};

#[cfg(unix)]
use crate::{child_process, storage::Storage};

const IDENTITY_KEYS: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "XDG_CONFIG_HOME",
    "USER_TYPE",
    "USE_LOCAL_OAUTH",
    "USE_STAGING_OAUTH",
    "CLAUDE_LOCAL_OAUTH_API_BASE",
    "CLAUDE_CODE_CUSTOM_OAUTH_URL",
];

static ENVIRONMENT: OnceLock<ProviderEnvironment> = OnceLock::new();

struct ProviderEnvironment {
    launch_snapshot: Option<HashMap<String, String>>,
}

pub fn initialize(launch_snapshot: Option<HashMap<String, String>>) {
    let launch_snapshot = if cfg!(unix) { launch_snapshot } else { None };
    let _ = ENVIRONMENT.set(ProviderEnvironment { launch_snapshot });
}

/// Returns provider configuration without ever persisting credentials.
///
/// The process environment always wins. On Unix, identity-sensitive values captured from the
/// login shell on the previous launch remain fixed for this session so provider IDs cannot change
/// while the app is running.
pub fn value(name: &str) -> Option<String> {
    process_value(name).or_else(|| {
        if !IDENTITY_KEYS.contains(&name) {
            return None;
        }
        ENVIRONMENT
            .get()
            .and_then(|environment| environment.launch_snapshot.as_ref())
            .and_then(|snapshot| snapshot.get(name).cloned())
    })
}

/// Extra Claude accounts should not be assembled from incomplete GUI-launch environment facts.
/// Unit tests that do not initialize the application environment retain the direct-env behavior.
pub fn claude_identity_facts_reliable() -> bool {
    identity_facts_reliable(
        cfg!(target_os = "windows"),
        process_value("CLAUDE_CONFIG_DIR").is_some(),
        ENVIRONMENT
            .get()
            .map(|environment| environment.launch_snapshot.is_some()),
    )
}

fn identity_facts_reliable(
    windows: bool,
    process_override_present: bool,
    initialized_snapshot: Option<bool>,
) -> bool {
    windows
        || process_override_present
        || initialized_snapshot.is_none_or(|snapshot_present| snapshot_present)
}

#[cfg(unix)]
pub fn refresh_for_next_launch(storage: Arc<Storage>) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(snapshot) = capture_login_shell_snapshot() {
            let _ = storage.save_provider_environment(&snapshot);
        }
    });
}

#[cfg(not(unix))]
pub fn refresh_for_next_launch(_storage: std::sync::Arc<crate::storage::Storage>) {}

fn process_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(unix)]
fn capture_login_shell_snapshot() -> Option<HashMap<String, String>> {
    const BEGIN: &str = "__OPENQUOTA_ENV_BEGIN__";
    const END: &str = "__OPENQUOTA_ENV_END__";

    let shell = process_value("SHELL").unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_owned()
        } else {
            "/bin/sh".to_owned()
        }
    });
    let script = format!("printf '%s\\0' '{BEGIN}'; /usr/bin/env -0; printf '%s\\0' '{END}'");
    let output = child_process::output_with_timeout(
        child_process::background_command(&shell).args(["-i", "-l", "-c", &script]),
        Duration::from_secs(5),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }

    parse_environment(&output.stdout, BEGIN, END).map(|environment| {
        IDENTITY_KEYS
            .iter()
            .filter_map(|key| {
                environment
                    .get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect()
    })
}

#[cfg(any(unix, test))]
fn parse_environment(
    bytes: &[u8],
    begin_marker: &str,
    end_marker: &str,
) -> Option<HashMap<String, String>> {
    let begin = format!("{begin_marker}\0");
    let end = format!("{end_marker}\0");
    let start = bytes
        .windows(begin.len())
        .position(|window| window == begin.as_bytes())?
        + begin.len();
    let relative_end = bytes[start..]
        .windows(end.len())
        .position(|window| window == end.as_bytes())?;
    let fields = &bytes[start..start + relative_end];
    let mut environment = HashMap::new();

    for field in fields.split(|byte| *byte == 0) {
        let text = String::from_utf8_lossy(field);
        if let Some((name, value)) = text.split_once('=') {
            if !name.is_empty() {
                environment.insert(name.to_owned(), value.to_owned());
            }
        }
    }

    Some(environment)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{identity_facts_reliable, parse_environment};

    #[test]
    fn claude_discovery_waits_only_for_a_cold_unix_shell_snapshot() {
        assert!(identity_facts_reliable(true, false, Some(false)));
        assert!(identity_facts_reliable(false, true, Some(false)));
        assert!(identity_facts_reliable(false, false, None));
        assert!(identity_facts_reliable(false, false, Some(true)));
        assert!(!identity_facts_reliable(false, false, Some(false)));
    }

    #[test]
    fn parses_only_environment_between_shell_markers() {
        let bytes = b"startup text\nbegin\0A=one\0EMPTY=\0B=two=three\0end\0trailing\0";
        let environment = parse_environment(bytes, "begin", "end").unwrap();

        assert_eq!(environment.get("A").map(String::as_str), Some("one"));
        assert_eq!(environment.get("EMPTY").map(String::as_str), Some(""));
        assert_eq!(environment.get("B").map(String::as_str), Some("two=three"));
        assert_eq!(environment.len(), 3);
        assert!(parse_environment(b"A=one\0", "begin", "end").is_none());
    }

    #[test]
    fn identity_snapshot_serializes_without_secret_keys() {
        let mut source = HashMap::new();
        source.insert("CLAUDE_CONFIG_DIR".to_owned(), "/tmp/claude".to_owned());
        source.insert("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), "secret".to_owned());

        let filtered = super::IDENTITY_KEYS
            .iter()
            .filter_map(|key| {
                source
                    .get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(
            filtered.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/tmp/claude")
        );
        assert!(!filtered.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
    }
}
