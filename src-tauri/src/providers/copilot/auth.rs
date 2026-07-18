use std::{
    collections::HashSet,
    fs,
    io::Read,
    ops::ControlFlow,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
use crate::providers::credential_store::read_generic_password;
use crate::{
    child_process::background_command, providers::credential_store::decode_go_keyring_value,
};

const GH_KEYRING_SERVICE: &str = "gh:github.com";
const GH_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 4096;

pub(super) struct CopilotToken(Zeroizing<String>);

impl CopilotToken {
    fn new(value: impl Into<String>) -> Option<Self> {
        let value = Zeroizing::new(value.into());
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.len() > MAX_TOKEN_BYTES
            || trimmed.chars().any(char::is_whitespace)
            || trimmed.chars().any(char::is_control)
        {
            return None;
        }
        if trimmed.len() == value.len() {
            Some(Self(value))
        } else {
            Some(Self(Zeroizing::new(trimmed.to_owned())))
        }
    }

    pub(super) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn fingerprint(&self) -> [u8; 32] {
        Sha256::digest(self.as_str().as_bytes()).into()
    }
}

trait TextFileAccess: Send + Sync {
    fn read_text(&self, path: &Path) -> Option<String>;
}

#[derive(Default)]
struct LocalTextFiles;

impl TextFileAccess for LocalTextFiles {
    fn read_text(&self, path: &Path) -> Option<String> {
        let metadata = fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
            return None;
        }
        fs::read_to_string(path).ok()
    }
}

trait GhTokenCommand: Send + Sync {
    fn token(&self) -> Option<CopilotToken>;
}

#[derive(Default)]
struct LocalGhTokenCommand;

impl GhTokenCommand for LocalGhTokenCommand {
    fn token(&self) -> Option<CopilotToken> {
        run_gh_token_command(GH_COMMAND_TIMEOUT)
    }
}

trait CredentialAccess: Send + Sync {
    fn read(&self, service: &str, account: &str) -> Option<Vec<u8>>;
    fn read_service(&self, service: &str) -> Option<Vec<u8>>;
}

#[derive(Default)]
struct SystemCredentials;

impl CredentialAccess for SystemCredentials {
    #[cfg(target_os = "macos")]
    fn read(&self, service: &str, account: &str) -> Option<Vec<u8>> {
        read_generic_password(service, account).ok().flatten()
    }

    #[cfg(target_os = "macos")]
    fn read_service(&self, service: &str) -> Option<Vec<u8>> {
        use security_framework::passwords::{generic_password, PasswordOptions};

        // `PasswordOptions` has no service-only constructor. Its generic-password
        // constructor appends the account constraint last, so removing that one
        // constraint yields the same service-scoped query used by GitHub CLI.
        let mut options = PasswordOptions::new_generic_password(service, "");
        #[allow(deprecated)]
        {
            options.query.pop()?;
        }
        generic_password(options).ok()
    }

    #[cfg(not(target_os = "macos"))]
    fn read(&self, _service: &str, _account: &str) -> Option<Vec<u8>> {
        // GitHub CLI's credential target is not a stable public contract on these
        // platforms. `gh auth token` above is the supported noninteractive accessor.
        None
    }

    #[cfg(not(target_os = "macos"))]
    fn read_service(&self, _service: &str) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone)]
struct AuthPaths {
    editor_configs: Vec<PathBuf>,
    gh_configs: Vec<PathBuf>,
}

impl AuthPaths {
    fn discover() -> Self {
        let home = home_directory();
        let mut editor_directories = Vec::new();
        if let Some(xdg) = environment_path("XDG_CONFIG_HOME") {
            push_unique(&mut editor_directories, xdg.join("github-copilot"));
        }
        push_unique(
            &mut editor_directories,
            home.join(".config").join("github-copilot"),
        );

        let mut editor_configs = Vec::new();
        for directory in editor_directories {
            push_unique(&mut editor_configs, directory.join("apps.json"));
            push_unique(&mut editor_configs, directory.join("hosts.json"));
        }

        let mut gh_configs = Vec::new();
        if let Some(directory) = environment_path("GH_CONFIG_DIR") {
            gh_configs.push(directory.join("hosts.yml"));
        } else {
            if let Some(xdg) = environment_path("XDG_CONFIG_HOME") {
                push_unique(&mut gh_configs, xdg.join("gh").join("hosts.yml"));
            }
            #[cfg(target_os = "windows")]
            if let Some(app_data) = environment_path("APPDATA") {
                push_unique(
                    &mut gh_configs,
                    app_data.join("GitHub CLI").join("hosts.yml"),
                );
            }
            push_unique(
                &mut gh_configs,
                home.join(".config").join("gh").join("hosts.yml"),
            );
        }

        Self {
            editor_configs,
            gh_configs,
        }
    }
}

pub(super) struct CopilotAuthStore {
    paths: AuthPaths,
    files: Arc<dyn TextFileAccess>,
    gh_command: Arc<dyn GhTokenCommand>,
    credentials: Arc<dyn CredentialAccess>,
}

impl CopilotAuthStore {
    pub(super) fn new() -> Self {
        Self {
            paths: AuthPaths::discover(),
            files: Arc::new(LocalTextFiles),
            gh_command: Arc::new(LocalGhTokenCommand),
            credentials: Arc::new(SystemCredentials),
        }
    }

    pub(super) fn visit_candidates<B>(
        &self,
        mut visit: impl FnMut(CopilotToken) -> ControlFlow<B>,
    ) -> Option<B> {
        let mut seen = HashSet::new();

        for path in &self.paths.editor_configs {
            let candidate = self
                .files
                .read_text(path)
                .and_then(|text| editor_oauth_token(&text))
                .and_then(CopilotToken::new);
            if let Some(result) = visit_candidate(candidate, &mut seen, &mut visit) {
                return Some(result);
            }
        }

        let gh_configs = self.gh_config_texts().collect::<Vec<_>>();
        for text in &gh_configs {
            let candidate = yaml_value(text, "oauth_token").and_then(CopilotToken::new);
            if let Some(result) = visit_candidate(candidate, &mut seen, &mut visit) {
                return Some(result);
            }
        }

        if let Some(result) = visit_candidate(self.gh_command.token(), &mut seen, &mut visit) {
            return Some(result);
        }

        for text in &gh_configs {
            let Some(account) = yaml_value(text, "user") else {
                continue;
            };
            let candidate = self
                .credentials
                .read(GH_KEYRING_SERVICE, &account)
                .and_then(|raw| token_from_keyring(&raw));
            if let Some(result) = visit_candidate(candidate, &mut seen, &mut visit) {
                return Some(result);
            }
        }

        let service_candidate = self
            .credentials
            .read_service(GH_KEYRING_SERVICE)
            .and_then(|raw| token_from_keyring(&raw));
        visit_candidate(service_candidate, &mut seen, &mut visit)
    }

    pub(super) fn load(&self) -> Option<CopilotToken> {
        self.visit_candidates(ControlFlow::Break)
    }

    pub(super) fn has_local_credentials(&self) -> bool {
        self.load().is_some()
    }

    fn gh_config_texts(&self) -> impl Iterator<Item = String> + '_ {
        self.paths
            .gh_configs
            .iter()
            .filter_map(|path| self.files.read_text(path))
    }

    #[cfg(test)]
    pub(super) fn for_test_token(token: Option<&str>) -> Self {
        match token {
            Some(token) => Self::for_test_tokens(&[token]),
            None => Self::for_test_tokens(&[]),
        }
    }

    #[cfg(test)]
    pub(super) fn for_test_tokens(tokens: &[&str]) -> Self {
        let values = tokens
            .iter()
            .enumerate()
            .map(|(index, token)| {
                let path = PathBuf::from(format!("editor-apps-{index}.json"));
                (
                    path,
                    format!(r#"{{"github.com":{{"oauth_token":"{token}"}}}}"#),
                )
            })
            .collect::<Vec<_>>();
        let editor_configs = values.iter().map(|(path, _)| path.clone()).collect();
        let files = MemoryFiles::from_pairs(values);
        Self {
            paths: AuthPaths {
                editor_configs,
                gh_configs: Vec::new(),
            },
            files: Arc::new(files),
            gh_command: Arc::new(NoGhCommand),
            credentials: Arc::new(NoCredentials),
        }
    }
}

fn visit_candidate<B>(
    candidate: Option<CopilotToken>,
    seen: &mut HashSet<[u8; 32]>,
    visit: &mut impl FnMut(CopilotToken) -> ControlFlow<B>,
) -> Option<B> {
    let candidate = candidate?;
    if !seen.insert(candidate.fingerprint()) {
        return None;
    }
    match visit(candidate) {
        ControlFlow::Break(result) => Some(result),
        ControlFlow::Continue(()) => None,
    }
}

impl Default for CopilotAuthStore {
    fn default() -> Self {
        Self::new()
    }
}

fn editor_oauth_token(text: &str) -> Option<String> {
    let object = serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .as_object()?
        .clone();
    object.into_iter().find_map(|(host, value)| {
        if host != "github.com" && !host.starts_with("github.com:") {
            return None;
        }
        value
            .get("oauth_token")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    })
}

fn yaml_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    let mut in_github = false;
    for line in text.lines() {
        if line
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            in_github = line.trim() == "github.com:";
            continue;
        }
        if !in_github {
            continue;
        }
        let trimmed = line.trim();
        let Some(raw) = trimmed.strip_prefix(&prefix) else {
            continue;
        };
        let raw = raw.trim();
        let raw = if raw.len() >= 2
            && ((raw.starts_with('"') && raw.ends_with('"'))
                || (raw.starts_with('\'') && raw.ends_with('\'')))
        {
            &raw[1..raw.len() - 1]
        } else {
            raw
        };
        let value = raw.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

fn token_from_keyring(raw: &[u8]) -> Option<CopilotToken> {
    let text = std::str::from_utf8(raw).ok()?.trim();
    if text.starts_with("go-keyring-base64:") {
        decode_go_keyring_value(raw).and_then(CopilotToken::new)
    } else {
        CopilotToken::new(text)
    }
}

fn run_gh_token_command(timeout: Duration) -> Option<CopilotToken> {
    let mut child = background_command("gh");
    child
        .args(["auth", "token", "--hostname", "github.com"])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = child.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((MAX_TOKEN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let bytes = receiver
        .recv_timeout(Duration::from_millis(100))
        .ok()?
        .ok()?;
    if bytes.len() > MAX_TOKEN_BYTES {
        return None;
    }
    let bytes = Zeroizing::new(bytes);
    let token = std::str::from_utf8(bytes.as_slice()).ok()?;
    CopilotToken::new(token)
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemoryFiles(std::collections::HashMap<PathBuf, String>);

#[cfg(test)]
impl MemoryFiles {
    fn from_pairs(values: Vec<(PathBuf, String)>) -> Self {
        Self(values.into_iter().collect())
    }
}

#[cfg(test)]
impl TextFileAccess for MemoryFiles {
    fn read_text(&self, path: &Path) -> Option<String> {
        self.0.get(path).cloned()
    }
}

#[cfg(test)]
struct NoGhCommand;

#[cfg(test)]
impl GhTokenCommand for NoGhCommand {
    fn token(&self) -> Option<CopilotToken> {
        None
    }
}

#[cfg(test)]
struct NoCredentials;

#[cfg(test)]
impl CredentialAccess for NoCredentials {
    fn read(&self, _service: &str, _account: &str) -> Option<Vec<u8>> {
        None
    }

    fn read_service(&self, _service: &str) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use base64::{engine::general_purpose::STANDARD, Engine};

    use super::{
        editor_oauth_token, token_from_keyring, yaml_value, AuthPaths, CopilotAuthStore,
        CopilotToken, CredentialAccess, GhTokenCommand, MemoryFiles,
    };

    enum FakeGhResult {
        Token(String),
        Failed,
        TimedOut,
    }

    struct FakeGh {
        result: FakeGhResult,
        calls: Mutex<usize>,
    }

    impl GhTokenCommand for FakeGh {
        fn token(&self) -> Option<CopilotToken> {
            *self.calls.lock().unwrap() += 1;
            match &self.result {
                FakeGhResult::Token(token) => CopilotToken::new(token.clone()),
                FakeGhResult::Failed | FakeGhResult::TimedOut => None,
            }
        }
    }

    struct FakeCredentials {
        values: HashMap<(String, String), Vec<u8>>,
        service_values: HashMap<String, Vec<u8>>,
        calls: Mutex<usize>,
        service_calls: Mutex<usize>,
    }

    impl CredentialAccess for FakeCredentials {
        fn read(&self, service: &str, account: &str) -> Option<Vec<u8>> {
            *self.calls.lock().unwrap() += 1;
            self.values
                .get(&(service.to_owned(), account.to_owned()))
                .cloned()
        }

        fn read_service(&self, service: &str) -> Option<Vec<u8>> {
            *self.service_calls.lock().unwrap() += 1;
            self.service_values.get(service).cloned()
        }
    }

    fn store(
        editor: &[(&str, &str)],
        gh: &[(&str, &str)],
        command: Arc<FakeGh>,
        credentials: Arc<FakeCredentials>,
    ) -> CopilotAuthStore {
        let editor_paths = editor
            .iter()
            .map(|(path, _)| PathBuf::from(path))
            .collect::<Vec<_>>();
        let gh_paths = gh
            .iter()
            .map(|(path, _)| PathBuf::from(path))
            .collect::<Vec<_>>();
        let files = editor
            .iter()
            .chain(gh)
            .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
            .collect::<Vec<_>>();
        CopilotAuthStore {
            paths: AuthPaths {
                editor_configs: editor_paths,
                gh_configs: gh_paths,
            },
            files: Arc::new(MemoryFiles::from_pairs(files)),
            gh_command: command,
            credentials,
        }
    }

    fn command(value: Option<&str>) -> Arc<FakeGh> {
        Arc::new(FakeGh {
            result: value
                .map(|value| FakeGhResult::Token(value.to_owned()))
                .unwrap_or(FakeGhResult::Failed),
            calls: Mutex::new(0),
        })
    }

    fn credentials(value: Option<(&str, &[u8])>) -> Arc<FakeCredentials> {
        Arc::new(FakeCredentials {
            values: value
                .map(|(account, value)| {
                    HashMap::from([(
                        ("gh:github.com".to_owned(), account.to_owned()),
                        value.to_vec(),
                    )])
                })
                .unwrap_or_default(),
            service_values: HashMap::new(),
            calls: Mutex::new(0),
            service_calls: Mutex::new(0),
        })
    }

    fn service_credentials(value: Option<&[u8]>) -> Arc<FakeCredentials> {
        Arc::new(FakeCredentials {
            values: HashMap::new(),
            service_values: value
                .map(|value| HashMap::from([("gh:github.com".to_owned(), value.to_vec())]))
                .unwrap_or_default(),
            calls: Mutex::new(0),
            service_calls: Mutex::new(0),
        })
    }

    #[test]
    fn editor_parser_uses_only_github_dot_com_entries() {
        assert_eq!(
            editor_oauth_token(
                r#"{"ghe.example:app":{"oauth_token":"enterprise"},
                    "github.com:app":{"oauth_token":"dotcom"}}"#
            )
            .as_deref(),
            Some("dotcom")
        );
        assert_eq!(
            editor_oauth_token(r#"{"ghe.example":{"oauth_token":"enterprise"}}"#),
            None
        );
        assert_eq!(editor_oauth_token("{broken"), None);
    }

    #[test]
    fn yaml_parser_is_scoped_to_github_dot_com_and_ignores_nested_users() {
        let text = r#"
ghe.example:
    oauth_token: enterprise
github.com:
    users:
        octocat:
    user: "octocat"
    oauth_token: 'dotcom'
"#;
        assert_eq!(yaml_value(text, "oauth_token").as_deref(), Some("dotcom"));
        assert_eq!(yaml_value(text, "user").as_deref(), Some("octocat"));
    }

    #[test]
    fn source_precedence_is_editor_then_gh_config_then_command_then_keyring() {
        let gh = command(Some("command-token"));
        let vault = credentials(Some(("octocat", b"vault-token")));
        let auth = store(
            &[(
                "apps.json",
                r#"{"github.com":{"oauth_token":"editor-token"}}"#,
            )],
            &[(
                "hosts.yml",
                "github.com:\n    user: octocat\n    oauth_token: config-token\n",
            )],
            gh.clone(),
            vault.clone(),
        );
        assert_eq!(auth.load().unwrap().as_str(), "editor-token");
        assert_eq!(*gh.calls.lock().unwrap(), 0);
        assert_eq!(*vault.calls.lock().unwrap(), 0);

        let gh = command(Some("command-token"));
        let auth = store(
            &[("apps.json", "{}")],
            &[(
                "hosts.yml",
                "github.com:\n    user: octocat\n    oauth_token: config-token\n",
            )],
            gh.clone(),
            vault,
        );
        assert_eq!(auth.load().unwrap().as_str(), "config-token");
        assert_eq!(*gh.calls.lock().unwrap(), 0);

        let gh = command(Some("command-token"));
        let vault = credentials(Some(("octocat", b"vault-token")));
        let auth = store(
            &[],
            &[("hosts.yml", "github.com:\n    user: octocat\n")],
            gh.clone(),
            vault.clone(),
        );
        assert_eq!(auth.load().unwrap().as_str(), "command-token");
        assert_eq!(*gh.calls.lock().unwrap(), 1);
        assert_eq!(*vault.calls.lock().unwrap(), 0);
    }

    #[test]
    fn failed_or_timed_out_gh_fallback_can_use_the_scoped_system_credential() {
        for result in [FakeGhResult::Failed, FakeGhResult::TimedOut] {
            let gh = Arc::new(FakeGh {
                result,
                calls: Mutex::new(0),
            });
            let wrapped = format!("go-keyring-base64:{}", STANDARD.encode("vault-token"));
            let vault = credentials(Some(("octocat", wrapped.as_bytes())));
            let auth = store(
                &[],
                &[("hosts.yml", "github.com:\n    user: octocat\n")],
                gh.clone(),
                vault.clone(),
            );

            assert_eq!(auth.load().unwrap().as_str(), "vault-token");
            assert_eq!(*gh.calls.lock().unwrap(), 1);
            assert_eq!(*vault.calls.lock().unwrap(), 1);
        }
    }

    #[test]
    fn service_scoped_keyring_fallback_works_without_a_github_username() {
        let vault = service_credentials(Some(b"vault-token"));
        let auth = store(&[], &[], command(None), vault.clone());

        assert_eq!(auth.load().unwrap().as_str(), "vault-token");
        assert_eq!(*vault.calls.lock().unwrap(), 0);
        assert_eq!(*vault.service_calls.lock().unwrap(), 1);
    }

    #[test]
    fn candidates_keep_source_order_and_deduplicate_token_values() {
        let gh = command(Some("shared-token"));
        let vault = service_credentials(Some(b"vault-token"));
        let auth = store(
            &[
                (
                    "apps.json",
                    r#"{"github.com":{"oauth_token":"editor-token"}}"#,
                ),
                (
                    "hosts.json",
                    r#"{"github.com":{"oauth_token":"shared-token"}}"#,
                ),
            ],
            &[("hosts.yml", "github.com:\n    oauth_token: config-token\n")],
            gh,
            vault,
        );
        let mut candidates = Vec::new();
        let completed: Option<()> = auth.visit_candidates(|token| {
            candidates.push(token.as_str().to_owned());
            std::ops::ControlFlow::Continue(())
        });

        assert!(completed.is_none());
        assert_eq!(
            candidates,
            [
                "editor-token",
                "shared-token",
                "config-token",
                "vault-token"
            ]
        );
    }

    #[test]
    fn wrapped_plain_and_invalid_tokens_are_handled_without_exposing_them() {
        let wrapped = format!("go-keyring-base64:{}", STANDARD.encode("wrapped-token"));
        assert_eq!(
            token_from_keyring(wrapped.as_bytes()).unwrap().as_str(),
            "wrapped-token"
        );
        assert_eq!(
            token_from_keyring(b" plain-token ").unwrap().as_str(),
            "plain-token"
        );
        assert!(token_from_keyring(b"go-keyring-base64:not-base64").is_none());
        assert!(CopilotToken::new("line1\nline2").is_none());
    }

    #[test]
    fn detection_and_refresh_share_the_same_usable_source() {
        let auth = CopilotAuthStore::for_test_token(Some("same-token"));
        assert!(auth.has_local_credentials());
        assert_eq!(auth.load().unwrap().as_str(), "same-token");

        let missing = CopilotAuthStore::for_test_token(None);
        assert!(!missing.has_local_credentials());
        assert!(missing.load().is_none());
    }
}
