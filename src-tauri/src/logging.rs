use std::{
    collections::HashSet,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        Mutex, OnceLock,
    },
};

use chrono::{SecondsFormat, Utc};
use regex::{Captures, Regex};

use crate::models::LogLevel;

pub const DEFAULT_MAX_BYTES: u64 = 10_000_000;

static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);
static LOGGER: OnceLock<AppLogger> = OnceLock::new();
static LOCAL_USAGE_FAILURES: OnceLock<Mutex<HashSet<(String, PathBuf)>>> = OnceLock::new();

pub fn init(path: PathBuf, level: LogLevel) {
    CURRENT_LEVEL.store(level as u8, Ordering::Relaxed);
    if LOGGER.get().is_some() {
        return;
    }
    let mut sink = LogFile::new(path, DEFAULT_MAX_BYTES);
    if let Err(error) = sink.open() {
        eprintln!("OpenQuota file log disabled: {error}");
    }
    let _ = LOGGER.set(AppLogger {
        sink: Mutex::new(sink),
    });
}

pub fn set_level(level: LogLevel) {
    CURRENT_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn current_level() -> LogLevel {
    LogLevel::from_severity(CURRENT_LEVEL.load(Ordering::Relaxed))
}

pub fn enabled(level: LogLevel) -> bool {
    level as u8 <= CURRENT_LEVEL.load(Ordering::Relaxed)
}

pub fn emit(level: LogLevel, tag: &str, arguments: fmt::Arguments<'_>) {
    if !enabled(level) {
        return;
    }
    let message = redact_log_message(&arguments.to_string());
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let line = format_line(&timestamp, level, tag, &message);

    #[cfg(debug_assertions)]
    eprintln!("{line}");

    let Some(logger) = LOGGER.get() else {
        return;
    };
    let Ok(mut sink) = logger.sink.lock() else {
        eprintln!("OpenQuota file log disabled: lock unavailable");
        return;
    };
    if let Err(error) = sink.append(&line) {
        eprintln!("OpenQuota file log disabled: {error}");
    }
}

fn format_line(timestamp: &str, level: LogLevel, tag: &str, message: &str) -> String {
    format!("{timestamp} [{}] [{tag}] {message}", level.log_label())
}

pub fn log_path() -> PathBuf {
    LOGGER
        .get()
        .and_then(|logger| {
            logger
                .sink
                .lock()
                .ok()
                .map(|sink| sink.path().to_path_buf())
        })
        .unwrap_or_else(default_log_path)
}

pub fn default_log_path() -> PathBuf {
    default_log_directory().join("OpenQuota.log")
}

fn default_log_directory() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("OpenQuota")
            .join("logs");
    }
    #[cfg(target_os = "macos")]
    {
        return home_directory()
            .unwrap_or_else(std::env::temp_dir)
            .join("Library")
            .join("Logs")
            .join("OpenQuota");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home_directory().map(|home| home.join(".local").join("state")))
            .unwrap_or_else(std::env::temp_dir)
            .join("openquota")
            .join("logs");
    }
    #[allow(unreachable_code)]
    std::env::temp_dir().join("OpenQuota").join("logs")
}

#[cfg(not(target_os = "windows"))]
fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn local_usage_file_failed(provider_id: &str, path: &Path) {
    if update_local_usage_failure(provider_id, path, true) {
        let tag = format!("plugin:{provider_id}");
        crate::app_warn!(
            &tag,
            "Could not read a local usage log; skipped it for this refresh"
        );
    }
}

pub fn local_usage_file_recovered(provider_id: &str, path: &Path) {
    update_local_usage_failure(provider_id, path, false);
}

fn update_local_usage_failure(provider_id: &str, path: &Path, failed: bool) -> bool {
    let failures = LOCAL_USAGE_FAILURES.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut failures) = failures.lock() else {
        return false;
    };
    let key = (provider_id.to_owned(), path.to_path_buf());
    if failed {
        failures.insert(key)
    } else {
        failures.remove(&key);
        false
    }
}

struct AppLogger {
    sink: Mutex<LogFile>,
}

pub struct LogFile {
    path: PathBuf,
    archive_path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
    opened: bool,
    disabled: bool,
}

impl LogFile {
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        let archive_path = archive_path(&path);
        Self {
            path,
            archive_path,
            max_bytes,
            file: None,
            size: 0,
            opened: false,
            disabled: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }

    pub fn open(&mut self) -> io::Result<()> {
        if self.opened || self.disabled {
            return Ok(());
        }
        self.opened = true;
        if let Err(error) = self.open_inner() {
            self.disable();
            return Err(error);
        }
        Ok(())
    }

    pub fn append(&mut self, line: &str) -> io::Result<()> {
        if self.disabled {
            return Ok(());
        }
        if !self.opened {
            self.open()?;
        }
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        if self.size.saturating_add(bytes.len() as u64) > self.max_bytes {
            if let Err(error) = self.rotate() {
                self.disable();
                return Err(error);
            }
        }
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        if let Err(error) = file.write_all(&bytes) {
            self.disable();
            return Err(error);
        }
        self.size = self.size.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn open_inner(&mut self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.size = fs::metadata(&self.path)
            .map(|value| value.len())
            .unwrap_or(0);
        if self.size > self.max_bytes {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.take();
        if self.archive_path.exists() {
            fs::remove_file(&self.archive_path)?;
        }
        if self.path.exists() {
            fs::rename(&self.path, &self.archive_path)?;
        }
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.size = 0;
        Ok(())
    }

    fn disable(&mut self) {
        self.file = None;
        self.disabled = true;
    }
}

fn archive_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("OpenQuota");
    let extension = path.extension().and_then(|value| value.to_str());
    let name = extension
        .map(|extension| format!("{stem}.1.{extension}"))
        .unwrap_or_else(|| format!("{stem}.1"));
    path.with_file_name(name)
}

pub fn redact_value(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    if characters.len() <= 12 {
        return "[REDACTED]".to_owned();
    }
    format!(
        "{}...{}",
        characters[..4].iter().collect::<String>(),
        characters[characters.len() - 4..]
            .iter()
            .collect::<String>()
    )
}

pub fn redact_url(url: &str) -> String {
    let Some((base, tail)) = url.split_once('?') else {
        return redact_url_authority(url);
    };
    let (query, fragment) = tail
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((tail, None));
    let query = query
        .split('&')
        .map(|parameter| {
            let Some((name, value)) = parameter.split_once('=') else {
                return parameter.to_owned();
            };
            if !value.is_empty() && sensitive_url_parameter(name) {
                format!("{name}={}", redact_value(value))
            } else {
                parameter.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    let fragment = fragment
        .map(|value| format!("#{value}"))
        .unwrap_or_default();
    format!("{}?{query}{fragment}", redact_url_authority(base))
}

fn redact_url_authority(url: &str) -> String {
    url_authority_regex()
        .replace_all(url, |captures: &Captures<'_>| {
            format!("{}[REDACTED]@", &captures[1])
        })
        .into_owned()
}

fn sensitive_url_parameter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "key",
        "api_key",
        "apikey",
        "token",
        "access_token",
        "secret",
        "password",
        "auth",
        "authorization",
        "bearer",
        "credential",
        "user",
        "user_id",
        "userid",
        "account_id",
        "accountid",
        "profilearn",
        "profile_arn",
        "email",
        "login",
    ]
    .iter()
    .any(|candidate| name.contains(candidate))
}

#[allow(dead_code)]
pub fn redact_body(body: &str) -> String {
    let redacted = json_sensitive_regex()
        .replace_all(body, |captures: &Captures<'_>| {
            format!("\"{}\": \"{}\"", &captures[1], redact_value(&captures[2]))
        })
        .into_owned();
    redact_log_message(&redacted)
}

#[allow(dead_code)]
pub fn body_preview(body: &str) -> String {
    body_preview_with_limit(body, 500)
}

fn body_preview_with_limit(body: &str, limit: usize) -> String {
    let redacted = redact_body(body);
    if redacted.len() <= limit {
        return redacted;
    }
    let end = redacted
        .char_indices()
        .take_while(|(index, _)| *index < limit)
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
        .min(redacted.len());
    format!("{}... ({} bytes total)", &redacted[..end], body.len())
}

pub fn redact_log_message(message: &str) -> String {
    let mut output = jwt_regex()
        .replace_all(message, |captures: &Captures<'_>| {
            redact_value(&captures[0])
        })
        .into_owned();
    output = api_key_regex()
        .replace_all(&output, |captures: &Captures<'_>| {
            redact_value(&captures[0])
        })
        .into_owned();
    output = devin_token_regex()
        .replace_all(&output, |captures: &Captures<'_>| {
            redact_value(&captures[0])
        })
        .into_owned();
    output = bearer_regex()
        .replace_all(&output, |captures: &Captures<'_>| {
            format!("{}{}", &captures[1], redact_value(&captures[2]))
        })
        .into_owned();
    output = named_secret_regex()
        .replace_all(&output, |captures: &Captures<'_>| {
            format!("{}={}", &captures[1], redact_value(&captures[2]))
        })
        .into_owned();
    output = account_regex()
        .replace_all(&output, |captures: &Captures<'_>| {
            format!("{}{}", &captures[1], redact_value(&captures[2]))
        })
        .into_owned();
    output = unix_path_regex()
        .replace_all(&output, "[PATH]")
        .into_owned();
    windows_path_regex()
        .replace_all(&output, "[PATH]")
        .into_owned()
}

fn jwt_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").unwrap())
}

fn api_key_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?:sk-|pk-|api_|key_|secret_)[A-Za-z0-9_-]{12,}").unwrap())
}

fn devin_token_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r#"devin-session-token\$[^\s"',}\]]+"#).unwrap())
}

fn bearer_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?i)(bearer\s+)([A-Za-z0-9._~+/=-]+)").unwrap())
}

fn named_secret_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(access_token|refresh_token|api_key|apikey|password|secret|session_token|authorization|email|user_id|account_id)=([^\s,;&]+)",
        )
        .unwrap()
    })
}

fn account_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(?i)(account=)([^,\s]+)").unwrap())
}

fn unix_path_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r#"/(?:Users|home|opt|private|var|tmp|Applications|mnt|run/user)/[^\s"')]+"#)
            .unwrap()
    })
}

fn windows_path_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r#"(?i)[A-Z]:(?:\\|/)[^\s"')]+"#).unwrap())
}

fn url_authority_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"(https?://)[^/@\s]+:[^/@\s]+@").unwrap())
}

fn json_sensitive_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r#"(?i)"(name|password|token|access_token|refresh_token|secret|api_key|apiKey|authorization|bearer|credential|session_token|sessionToken|auth_token|authToken|id_token|idToken|accessToken|refreshToken|user_id|userId|account_id|accountId|team_id|teamId|org_id|orgId|account_display_name|accountDisplayName|payment_id|paymentId|profile_arn|profileArn|email|login|analytics_tracking_id)"\s*:\s*"([^"]*)""#,
        )
        .unwrap()
    })
}

#[macro_export]
macro_rules! app_error {
    ($tag:expr, $($argument:tt)*) => {{
        if $crate::logging::enabled($crate::models::LogLevel::Error) {
            $crate::logging::emit(
                $crate::models::LogLevel::Error,
                $tag,
                format_args!($($argument)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! app_warn {
    ($tag:expr, $($argument:tt)*) => {{
        if $crate::logging::enabled($crate::models::LogLevel::Warn) {
            $crate::logging::emit(
                $crate::models::LogLevel::Warn,
                $tag,
                format_args!($($argument)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! app_info {
    ($tag:expr, $($argument:tt)*) => {{
        if $crate::logging::enabled($crate::models::LogLevel::Info) {
            $crate::logging::emit(
                $crate::models::LogLevel::Info,
                $tag,
                format_args!($($argument)*),
            );
        }
    }};
}

#[macro_export]
macro_rules! app_debug {
    ($tag:expr, $($argument:tt)*) => {{
        if $crate::logging::enabled($crate::models::LogLevel::Debug) {
            $crate::logging::emit(
                $crate::models::LogLevel::Debug,
                $tag,
                format_args!($($argument)*),
            );
        }
    }};
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicBool, Ordering},
    };

    use tempfile::tempdir;

    use super::{
        body_preview, default_log_path, format_line, redact_body, redact_log_message, redact_url,
        redact_value, update_local_usage_failure, LogFile,
    };

    #[test]
    fn redacts_short_and_long_values() {
        assert_eq!(redact_value("short"), "[REDACTED]");
        assert_eq!(redact_value("abcdefghijkl"), "[REDACTED]");
        assert_eq!(redact_value("sk-1234567890abcdef"), "sk-1...cdef");
    }

    #[test]
    fn redacts_sensitive_url_parts_without_touching_safe_parameters() {
        let redacted =
            redact_url("https://user:password@example.com/v1?api_key=sk-1234567890abcdef&limit=10");
        assert!(!redacted.contains("password"));
        assert!(redacted.contains("api_key=sk-1...cdef"));
        assert!(redacted.contains("limit=10"));
    }

    #[test]
    fn redacts_body_secrets_before_truncating() {
        let body = format!(
            "{{\"email\":\"person@example.com\",\"token\":\"{}\"}}{}",
            "eyJheader.payload.signature",
            "x".repeat(600)
        );
        let redacted = redact_body(&body);
        assert!(!redacted.contains("person@example.com"));
        assert!(!redacted.contains("eyJheader.payload.signature"));
        let preview = body_preview(&body);
        assert!(!preview.contains("person@example.com"));
        assert!(preview.ends_with(&format!("... ({} bytes total)", body.len())));
    }

    #[test]
    fn redacts_free_form_tokens_accounts_and_cross_platform_paths() {
        let redacted = redact_log_message(
            r"Bearer abcdefghijklmnopqrstuvwxyz account=someone path=/Users/someone/.claude other=C:\Users\someone\.codex key=sk-1234567890abcdef",
        );
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("account=someone"));
        assert!(!redacted.contains("/Users/someone"));
        assert!(!redacted.contains(r"C:\Users\someone"));
        assert!(!redacted.contains("sk-1234567890abcdef"));
        assert!(redacted.contains("[PATH]"));
    }

    #[test]
    fn append_writes_grep_friendly_lines() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("OpenQuota.log");
        let mut sink = LogFile::new(path.clone(), 1_000);
        sink.open().unwrap();
        sink.append("2026-01-01T00:00:00.000Z [INFO] [config] hello")
            .unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("[INFO] [config] hello"));
        assert!(contents.ends_with('\n'));
    }

    #[test]
    fn formatted_line_carries_timestamp_level_and_category() {
        assert_eq!(
            format_line(
                "2026-07-15T12:00:00.000Z",
                crate::models::LogLevel::Warn,
                "auth:codex",
                "token refresh failed"
            ),
            "2026-07-15T12:00:00.000Z [WARN] [auth:codex] token refresh failed"
        );
    }

    #[test]
    fn default_path_uses_the_platform_log_directory_and_advertised_file_name() {
        let path = default_log_path();
        assert_eq!(
            path.file_name().and_then(|value| value.to_str()),
            Some("OpenQuota.log")
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            path.parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str()),
            Some("logs")
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            path.parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str()),
            Some("OpenQuota")
        );
        #[cfg(target_os = "macos")]
        assert!(path.to_string_lossy().contains("Library/Logs/OpenQuota"));
        #[cfg(target_os = "windows")]
        assert!(path.to_string_lossy().contains("OpenQuota\\logs"));
        #[cfg(all(unix, not(target_os = "macos")))]
        assert!(path.to_string_lossy().contains("openquota/logs"));
    }

    #[test]
    fn rotation_keeps_one_archive_and_trims_oversize_on_open() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("OpenQuota.log");
        let mut sink = LogFile::new(path.clone(), 200);
        sink.open().unwrap();
        let line = "a".repeat(80);
        for _ in 0..10 {
            sink.append(&line).unwrap();
        }
        assert!(sink.archive_path().exists());
        assert!(!directory.path().join("OpenQuota.2.log").exists());

        drop(sink);
        fs::write(&path, vec![b'x'; 250]).unwrap();
        let mut reopened = LogFile::new(path.clone(), 200);
        reopened.open().unwrap();
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
        assert_eq!(fs::metadata(reopened.archive_path()).unwrap().len(), 250);
    }

    #[test]
    fn sink_failure_disables_file_logging_without_panicking() {
        let directory = tempdir().unwrap();
        let blocked_parent = directory.path().join("not-a-directory");
        fs::write(&blocked_parent, b"file").unwrap();
        let mut sink = LogFile::new(blocked_parent.join("OpenQuota.log"), 200);
        assert!(sink.open().is_err());
        assert!(sink.append("ignored after disable").is_ok());
    }

    #[test]
    fn debug_macro_does_not_build_message_at_info_floor() {
        super::set_level(crate::models::LogLevel::Info);
        let built = AtomicBool::new(false);
        fn expensive(flag: &AtomicBool) -> &'static str {
            flag.store(true, Ordering::SeqCst);
            "expensive"
        }
        crate::app_debug!("cache", "{}", expensive(&built));
        assert!(!built.load(Ordering::SeqCst));
    }

    #[test]
    fn local_usage_read_failure_warns_once_until_recovery() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("unique-session.jsonl");
        assert!(update_local_usage_failure("dedupe-test", &path, true));
        assert!(!update_local_usage_failure("dedupe-test", &path, true));
        assert!(!update_local_usage_failure("dedupe-test", &path, false));
        assert!(update_local_usage_failure("dedupe-test", &path, true));
    }
}
