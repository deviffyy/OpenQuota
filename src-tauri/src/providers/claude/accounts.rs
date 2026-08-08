use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::auth::{self, ClaudeCredentialScope};
use crate::{
    providers::credential_store::generic_password_service_exists,
    storage::{Storage, StorageError},
};

const DISCOVERY_BUDGET: Duration = Duration::from_millis(400);

#[derive(Debug, Clone)]
pub(super) struct ClaudeAccountDiscovery {
    pub default_account: Option<ClaudeAccount>,
    pub accounts: Vec<ClaudeAccount>,
}

#[derive(Debug, Clone)]
pub(super) struct ClaudeAccount {
    pub id: String,
    pub display_name: String,
    pub label: Option<String>,
    pub identity: String,
    pub credential_scope: ClaudeCredentialScope,
    pub log_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct DiscoveredClaudeAccounts {
    default_account: Option<DiscoveredClaudeAccount>,
    accounts: Vec<DiscoveredClaudeAccount>,
}

#[derive(Debug, Clone)]
struct DiscoveredClaudeAccount {
    label: Option<String>,
    identity: String,
    credential_scope: ClaudeCredentialScope,
    log_roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredClaudeAccountPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DefaultAccount {
    Resolved {
        identity: String,
        label: Option<String>,
    },
    Unresolved,
    Absent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeStateFile {
    oauth_account: Option<ClaudeOAuthAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOAuthAccount {
    account_uuid: Option<String>,
    email_address: Option<String>,
    organization_uuid: Option<String>,
    organization_name: Option<String>,
}

#[derive(Debug, Clone)]
struct Finding {
    identity: String,
    label: Option<String>,
    root: PathBuf,
    keychain_literal: String,
}

struct StoredAccountRecord {
    provider_id: String,
    label: Option<String>,
}

pub(super) fn discover(storage: &Storage) -> Result<ClaudeAccountDiscovery, StorageError> {
    if !crate::provider_environment::claude_identity_facts_reliable() {
        crate::app_info!(
            "config",
            "claude account discovery deferred until login-shell settings are available"
        );
        return Ok(ClaudeAccountDiscovery {
            default_account: None,
            accounts: Vec::new(),
        });
    }
    let home = home_directory();
    let config = env_text("CLAUDE_CONFIG_DIR");
    let xdg = env_text("XDG_CONFIG_HOME");
    reconcile_accounts(
        storage,
        discover_in(&home, config.as_deref(), xdg.as_deref(), DISCOVERY_BUDGET),
    )
}

pub(super) fn identity_for_scope(scope: &ClaudeCredentialScope) -> Option<String> {
    let home = home_directory();
    let identity = match scope {
        ClaudeCredentialScope::Standard => {
            match observe_default(&home, env_text("CLAUDE_CONFIG_DIR").as_deref()) {
                DefaultAccount::Resolved { identity, .. } => Some(identity),
                DefaultAccount::Unresolved | DefaultAccount::Absent => None,
            }
        }
        ClaudeCredentialScope::ConfigDir { path, .. } => fs::read(path.join(".claude.json"))
            .ok()
            .and_then(|bytes| parse_identity(&bytes))
            .map(|(identity, _)| identity),
    };
    identity.map(|identity| identity_stamp(&identity))
}

fn reconcile_accounts(
    storage: &Storage,
    discovery: DiscoveredClaudeAccounts,
) -> Result<ClaudeAccountDiscovery, StorageError> {
    let stored = storage.load_provider_account_records("claude")?;
    let records = stored
        .into_iter()
        .map(|(identity, provider_id, payload)| {
            let label = serde_json::from_str::<StoredClaudeAccountPayload>(&payload)
                .ok()
                .and_then(|account| account.label);
            (identity, StoredAccountRecord { provider_id, label })
        })
        .collect::<BTreeMap<_, _>>();
    let mut occupied = records
        .values()
        .map(|record| record.provider_id.clone())
        .collect::<HashSet<_>>();
    let default_account = discovery
        .default_account
        .map(|account| reconcile_account(storage, account, &records, &mut occupied, true))
        .transpose()?;
    let mut accounts = discovery
        .accounts
        .into_iter()
        .map(|account| reconcile_account(storage, account, &records, &mut occupied, false))
        .collect::<Result<Vec<_>, _>>()?;
    accounts.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ClaudeAccountDiscovery {
        default_account,
        accounts,
    })
}

fn reconcile_account(
    storage: &Storage,
    account: DiscoveredClaudeAccount,
    records: &BTreeMap<String, StoredAccountRecord>,
    occupied: &mut HashSet<String>,
    may_claim_default_id: bool,
) -> Result<ClaudeAccount, StorageError> {
    // IDs belong to accounts rather than credential locations. Existing accounts keep their ID
    // when they move, and only a newly observed default-home account may claim the bare ID.
    let record = records.get(&account.identity);
    let label = account
        .label
        .or_else(|| record.and_then(|record| record.label.clone()));
    let id = record
        .map(|record| record.provider_id.clone())
        .unwrap_or_else(|| {
            if may_claim_default_id && !occupied.contains("claude") {
                "claude".to_owned()
            } else {
                allocate_account_id(&account.identity, occupied)
            }
        });
    occupied.insert(id.clone());
    let reconciled = ClaudeAccount {
        display_name: account_display_name_for_id(label.as_deref(), &id),
        id,
        label,
        identity: account.identity,
        credential_scope: account.credential_scope,
        log_roots: account.log_roots,
    };
    let payload = serde_json::to_string(&StoredClaudeAccountPayload {
        label: reconciled.label.clone(),
    })?;
    storage.save_provider_account_record(
        "claude",
        &reconciled.identity,
        &reconciled.id,
        &payload,
    )?;
    Ok(reconciled)
}

fn allocate_account_id(identity_stamp: &str, occupied: &HashSet<String>) -> String {
    for salt in 0_u64.. {
        let stamp = if salt == 0 {
            identity_stamp.to_owned()
        } else {
            format!(
                "{:x}",
                Sha256::digest(format!("{identity_stamp}:{salt}").as_bytes())
            )
        };
        let candidate = format!("claude@{}", &stamp[..8]);
        if !occupied.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("an account ID is always available")
}

fn discover_in(
    home: &Path,
    config: Option<&str>,
    xdg: Option<&str>,
    budget: Duration,
) -> DiscoveredClaudeAccounts {
    let default = observe_default(home, config);
    let default_identity = match &default {
        DefaultAccount::Resolved { identity, .. } => Some(identity.clone()),
        DefaultAccount::Unresolved | DefaultAccount::Absent => None,
    };
    if default == DefaultAccount::Unresolved {
        crate::app_info!(
            "config",
            "claude extra-account discovery skipped because the default login identity is unreadable"
        );
        return DiscoveredClaudeAccounts {
            default_account: None,
            accounts: Vec::new(),
        };
    }

    let started = Instant::now();
    let deadline = started.checked_add(budget).unwrap_or(started);
    let excluded = default_roots(home, config, xdg)
        .into_iter()
        .map(|path| canonical(&path))
        .collect::<HashSet<_>>();
    let mut findings = Vec::new();
    for candidate in candidate_directories(home) {
        if started.elapsed() > budget {
            crate::app_info!(
                "config",
                "claude extra-account discovery reached its time budget"
            );
            break;
        }
        if excluded.contains(&canonical(&candidate)) {
            continue;
        }
        if let Some(finding) = inspect_candidate(home, &candidate, deadline) {
            findings.push(finding);
        }
    }

    let mut grouped = BTreeMap::<String, Vec<Finding>>::new();
    for finding in findings {
        grouped
            .entry(finding.identity.clone())
            .or_default()
            .push(finding);
    }

    let mut default_extra_log_roots = Vec::new();
    let mut accounts = Vec::new();
    for (identity, mut findings) in grouped {
        findings.sort_by(|left, right| left.root.cmp(&right.root));
        if default_identity.as_deref() == Some(identity.as_str()) {
            default_extra_log_roots.extend(findings.into_iter().map(|finding| finding.root));
            continue;
        }
        let Some(primary) = findings.first() else {
            continue;
        };
        accounts.push(DiscoveredClaudeAccount {
            label: primary.label.clone(),
            identity: identity_stamp(&identity),
            credential_scope: ClaudeCredentialScope::ConfigDir {
                path: primary.root.clone(),
                keychain_literal: primary.keychain_literal.clone(),
            },
            log_roots: findings.into_iter().map(|finding| finding.root).collect(),
        });
    }
    default_extra_log_roots.sort();
    default_extra_log_roots.dedup();

    if !accounts.is_empty() || !default_extra_log_roots.is_empty() {
        crate::app_info!(
            "config",
            "claude account discovery completed ({} extra account(s), {} folded log root(s))",
            accounts.len(),
            default_extra_log_roots.len()
        );
    }
    let default_account = match default {
        DefaultAccount::Resolved { identity, label } => Some(DiscoveredClaudeAccount {
            label,
            identity: identity_stamp(&identity),
            credential_scope: ClaudeCredentialScope::Standard,
            log_roots: default_extra_log_roots,
        }),
        DefaultAccount::Unresolved | DefaultAccount::Absent => None,
    };
    DiscoveredClaudeAccounts {
        default_account,
        accounts,
    }
}

fn observe_default(home: &Path, config: Option<&str>) -> DefaultAccount {
    let configured = config.map(str::trim).filter(|value| !value.is_empty());
    if configured.is_some_and(|value| value.contains(',')) {
        return DefaultAccount::Unresolved;
    }
    let root = configured
        .map(|value| expand_home(value, home))
        .unwrap_or_else(|| home.join(".claude"));
    let identity_path = if canonical(&root) == canonical(&home.join(".claude")) {
        home.join(".claude.json")
    } else {
        root.join(".claude.json")
    };
    let state = match fs::read(&identity_path) {
        Ok(bytes) => parse_identity(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return if root.join(".credentials.json").is_file() {
                DefaultAccount::Unresolved
            } else {
                DefaultAccount::Absent
            };
        }
        Err(_) => return DefaultAccount::Unresolved,
    };
    state
        .map(|(identity, label)| DefaultAccount::Resolved { identity, label })
        .unwrap_or(DefaultAccount::Unresolved)
}

fn inspect_candidate(home: &Path, root: &Path, deadline: Instant) -> Option<Finding> {
    let state = fs::read(root.join(".claude.json")).ok()?;
    let (identity, label) = parse_identity(&state)?;
    let file_backed = fs::read(root.join(".credentials.json"))
        .ok()
        .is_some_and(|bytes| auth::credentials_have_access_token(&bytes));

    let mut matched_literal = None;
    if should_probe_credential_store(file_backed) {
        for literal in keychain_literals(home, root) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let service = auth::scoped_keychain_service_name(&literal);
            let exists = generic_password_service_exists(&service, remaining) == Some(true);
            if exists {
                matched_literal = Some(literal);
                break;
            }
        }
    }
    if !file_backed && matched_literal.is_none() {
        return None;
    }
    Some(Finding {
        identity,
        label,
        root: canonical(root),
        keychain_literal: matched_literal.unwrap_or_else(|| root.to_string_lossy().into_owned()),
    })
}

fn should_probe_credential_store(file_backed: bool) -> bool {
    cfg!(target_os = "macos") || !file_backed
}

fn parse_identity(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let state: ClaudeStateFile = serde_json::from_slice(bytes).ok()?;
    let account = state.oauth_account?;
    let account_uuid = account
        .account_uuid
        .and_then(nonempty)?
        .to_ascii_lowercase();
    let organization_uuid = account
        .organization_uuid
        .and_then(nonempty)
        .map(|value| value.to_ascii_lowercase());
    let identity = organization_uuid
        .map(|organization| format!("{account_uuid}|{organization}"))
        .unwrap_or(account_uuid);
    let email = account.email_address.and_then(nonempty);
    let organization = account.organization_name.and_then(nonempty);
    let label = match (email, organization) {
        (Some(email), Some(organization)) => Some(format!("{email} ({organization})")),
        (Some(email), None) => Some(email),
        (None, Some(organization)) => Some(organization),
        (None, None) => None,
    };
    Some((identity, label))
}

fn account_display_name(label: Option<&str>, id: &str) -> String {
    let Some(label) = label.map(str::trim).filter(|value| !value.is_empty()) else {
        return id.to_owned();
    };
    if let Some(organization) = label
        .strip_suffix(')')
        .and_then(|value| value.rsplit_once('('))
    {
        let organization = organization.1.trim();
        if !organization.is_empty() {
            return format!("Claude — {organization}");
        }
    }
    format!("Claude — {label}")
}

fn account_display_name_for_id(label: Option<&str>, id: &str) -> String {
    if id == "claude" {
        "Claude".to_owned()
    } else {
        account_display_name(label, id)
    }
}

fn identity_stamp(identity: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(identity.to_ascii_lowercase().as_bytes())
    )
}

fn candidate_directories(home: &Path) -> Vec<PathBuf> {
    let mut candidates = child_directories(home)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect::<Vec<_>>();
    candidates.extend(child_directories(&home.join(".config")));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn child_directories(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn default_roots(home: &Path, config: Option<&str>, xdg: Option<&str>) -> Vec<PathBuf> {
    if let Some(config) = config.map(str::trim).filter(|value| !value.is_empty()) {
        let roots = config
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| expand_home(value, home))
            .collect::<Vec<_>>();
        if !roots.is_empty() {
            return roots;
        }
    }
    let xdg = xdg
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| expand_home(value, home))
        .unwrap_or_else(|| home.join(".config"));
    vec![xdg.join("claude"), home.join(".claude")]
}

fn keychain_literals(home: &Path, root: &Path) -> Vec<String> {
    let mut home_paths = vec![home.to_path_buf(), canonical(home)];
    let mut seen_paths = HashSet::new();
    home_paths.retain(|path| seen_paths.insert(path.clone()));
    let mut candidates = vec![root.to_path_buf(), canonical(root)];
    for candidate in candidates.clone() {
        for home_path in &home_paths {
            let Ok(relative) = candidate.strip_prefix(home_path) else {
                continue;
            };
            candidates.extend(home_paths.iter().map(|path| path.join(relative)));
        }
    }
    seen_paths.clear();
    candidates.retain(|path| seen_paths.insert(path.clone()));

    let mut literals = Vec::new();
    for candidate in candidates {
        let text = candidate.to_string_lossy().into_owned();
        literals.push(text);
        for home_path in &home_paths {
            if let Ok(relative) = candidate.strip_prefix(home_path) {
                let relative = relative.to_string_lossy().replace('\\', "/");
                literals.push(format!("~/{}", relative.trim_start_matches('/')));
            }
        }
    }
    let mut seen = HashSet::new();
    literals.retain(|literal| seen.insert(literal.clone()));
    literals
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
        .map(|rest| home.join(rest))
        .unwrap_or_else(|| PathBuf::from(value))
}

fn nonempty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn env_text(name: &str) -> Option<String> {
    crate::provider_environment::value(name)
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs, time::Duration};

    use tempfile::tempdir;

    use super::{
        allocate_account_id, canonical, discover_in, identity_stamp, keychain_literals,
        reconcile_accounts, should_probe_credential_store, DiscoveredClaudeAccount,
        DiscoveredClaudeAccounts,
    };
    use crate::{providers::claude::auth::ClaudeCredentialScope, storage::Storage};

    fn write_account(root: &std::path::Path, account: &str, organization: &str, email: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join(".claude.json"),
            format!(
                r#"{{"oauthAccount":{{"accountUuid":"{account}","organizationUuid":"{organization}","organizationName":"Org {organization}","emailAddress":"{email}"}}}}"#
            ),
        )
        .unwrap();
        fs::write(
            root.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"token"}}"#,
        )
        .unwrap();
    }

    #[test]
    fn discovers_distinct_config_accounts_and_folds_the_default_identity() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        write_account(
            &home.join(".claude-work"),
            "account-b",
            "org-b",
            "b@example.com",
        );
        write_account(
            &home.join(".claude-copy"),
            "account-a",
            "org-a",
            "a@example.com",
        );
        fs::write(
            home.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"account-a","organizationUuid":"org-a"}}"#,
        )
        .unwrap();

        let discovery = discover_in(home, None, None, Duration::from_secs(5));

        let default = discovery.default_account.as_ref().unwrap();
        assert_eq!(default.log_roots, [canonical(&home.join(".claude-copy"))]);
        assert_eq!(discovery.accounts.len(), 1);
        assert_eq!(
            discovery.accounts[0].identity,
            identity_stamp("account-b|org-b")
        );
        assert_eq!(
            discovery.accounts[0].label.as_deref(),
            Some("b@example.com (Org org-b)")
        );
        assert_eq!(
            discovery.accounts[0].log_roots,
            [canonical(&home.join(".claude-work"))]
        );
    }

    #[test]
    fn unresolved_default_identity_suppresses_extra_cards() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(home.join(".claude/.credentials.json"), b"{}").unwrap();
        write_account(
            &home.join(".claude-work"),
            "account-b",
            "org-b",
            "b@example.com",
        );

        let discovery = discover_in(home, None, None, Duration::from_secs(5));

        assert!(discovery.default_account.is_none());
        assert!(discovery.accounts.is_empty());
    }

    #[test]
    fn candidate_requires_both_a_named_account_and_reusable_credentials() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        let no_identity = home.join(".claude-no-identity");
        fs::create_dir_all(&no_identity).unwrap();
        fs::write(no_identity.join(".claude.json"), r#"{"oauthAccount":{}}"#).unwrap();
        fs::write(
            no_identity.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":"token"}}"#,
        )
        .unwrap();
        let no_credential = home.join(".claude-no-credential");
        fs::create_dir_all(&no_credential).unwrap();
        fs::write(
            no_credential.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"account-a"}}"#,
        )
        .unwrap();
        fs::write(
            no_credential.join(".credentials.json"),
            br#"{"claudeAiOauth":{"accessToken":" "}}"#,
        )
        .unwrap();

        let discovery = discover_in(home, None, None, Duration::from_millis(100));

        assert!(discovery.accounts.is_empty());
    }

    #[test]
    fn account_id_is_stable_and_does_not_expose_identity() {
        let id = allocate_account_id(&identity_stamp("Account-A|Org-A"), &HashSet::new());
        assert_eq!(
            id,
            allocate_account_id(&identity_stamp("account-a|org-a"), &HashSet::new())
        );
        assert!(id.starts_with("claude@"));
        assert_eq!(id.len(), "claude@".len() + 8);
        assert!(!id.contains("account"));
    }

    #[test]
    fn account_id_is_salted_when_the_short_hash_is_occupied() {
        let identity = "1234567890abcdef";
        let occupied = HashSet::from(["claude@12345678".to_owned()]);

        let collision_safe = allocate_account_id(identity, &occupied);
        assert!(collision_safe.starts_with("claude@"));
        assert_eq!(collision_safe.len(), "claude@".len() + 8);
        assert_ne!(collision_safe, "claude@12345678");
        assert_eq!(
            allocate_account_id(identity, &HashSet::new()),
            "claude@12345678"
        );
    }

    #[test]
    fn reconciled_account_keeps_its_id_but_is_not_rendered_when_absent() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let root = directory.path().join("account");
        let account = |label: &str| DiscoveredClaudeAccount {
            label: Some(label.into()),
            identity: "1234567890abcdef".into(),
            credential_scope: ClaudeCredentialScope::ConfigDir {
                path: root.clone(),
                keychain_literal: root.to_string_lossy().into_owned(),
            },
            log_roots: vec![root.clone()],
        };
        let discovery = |account| DiscoveredClaudeAccounts {
            default_account: None,
            accounts: vec![account],
        };

        let first = reconcile_accounts(&storage, discovery(account("First"))).unwrap();
        let second = reconcile_accounts(&storage, discovery(account("Updated"))).unwrap();
        assert_eq!(first.accounts[0].id, second.accounts[0].id);
        assert_eq!(second.accounts[0].display_name, "Claude — Updated");

        let mut without_label = account("throwaway");
        without_label.label = None;
        let without_label = reconcile_accounts(&storage, discovery(without_label)).unwrap();
        assert_eq!(without_label.accounts[0].label.as_deref(), Some("Updated"));
        assert_eq!(without_label.accounts[0].display_name, "Claude — Updated");

        let absent = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: None,
                accounts: Vec::new(),
            },
        )
        .unwrap();
        assert!(absent.accounts.is_empty());
    }

    #[test]
    fn default_account_keeps_its_last_descriptive_label() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let account = |label| DiscoveredClaudeAccount {
            label,
            identity: "1234567890abcdef".into(),
            credential_scope: ClaudeCredentialScope::Standard,
            log_roots: Vec::new(),
        };

        reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: Some(account(Some("Personal".into()))),
                accounts: Vec::new(),
            },
        )
        .unwrap();
        let without_label = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: Some(account(None)),
                accounts: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(
            without_label.default_account.unwrap().label.as_deref(),
            Some("Personal")
        );
    }

    #[test]
    fn legacy_account_payload_is_read_and_rewritten_without_runtime_paths() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let identity = identity_stamp("account-a");
        storage
            .save_provider_account_record(
                "claude",
                &identity,
                "claude@1234abcd",
                r#"{"id":"claude@1234abcd","display_name":"Claude — Saved","label":"Saved","log_roots":["/private/path"]}"#,
            )
            .unwrap();

        let reconciled = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: None,
                accounts: vec![DiscoveredClaudeAccount {
                    label: None,
                    identity,
                    credential_scope: ClaudeCredentialScope::Standard,
                    log_roots: Vec::new(),
                }],
            },
        )
        .unwrap();

        assert_eq!(reconciled.accounts[0].display_name, "Claude — Saved");
        let records = storage.load_provider_account_records("claude").unwrap();
        assert_eq!(records[0].2, r#"{"label":"Saved"}"#);
    }

    #[test]
    fn account_ids_survive_default_and_config_dir_swaps() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let root = directory.path().join("account");
        let standard = |identity: &str, label: &str| DiscoveredClaudeAccount {
            label: Some(label.into()),
            identity: identity_stamp(identity),
            credential_scope: ClaudeCredentialScope::Standard,
            log_roots: Vec::new(),
        };
        let scoped = |identity: &str, label: &str| DiscoveredClaudeAccount {
            credential_scope: ClaudeCredentialScope::ConfigDir {
                path: root.clone(),
                keychain_literal: root.to_string_lossy().into_owned(),
            },
            log_roots: vec![root.clone()],
            ..standard(identity, label)
        };

        let first = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: Some(standard("account-a", "Personal")),
                accounts: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(first.default_account.unwrap().id, "claude");

        let swapped = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: Some(standard("account-b", "Work")),
                accounts: vec![scoped("account-a", "Personal")],
            },
        )
        .unwrap();
        let account_b_id = swapped.default_account.as_ref().unwrap().id.clone();
        assert!(account_b_id.starts_with("claude@"));
        assert_eq!(swapped.accounts[0].id, "claude");
        assert_eq!(swapped.accounts[0].display_name, "Claude");

        let restored = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: Some(standard("account-a", "Personal")),
                accounts: vec![scoped("account-b", "Work")],
            },
        )
        .unwrap();
        assert_eq!(restored.default_account.unwrap().id, "claude");
        assert_eq!(restored.accounts[0].id, account_b_id);
    }

    #[test]
    fn config_dir_only_account_never_claims_the_bare_id() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let root = directory.path().join("account");
        let discovery = reconcile_accounts(
            &storage,
            DiscoveredClaudeAccounts {
                default_account: None,
                accounts: vec![DiscoveredClaudeAccount {
                    label: Some("Work".into()),
                    identity: identity_stamp("account-b"),
                    credential_scope: ClaudeCredentialScope::ConfigDir {
                        path: root.clone(),
                        keychain_literal: root.to_string_lossy().into_owned(),
                    },
                    log_roots: vec![root],
                }],
            },
        )
        .unwrap();

        assert!(discovery.accounts[0].id.starts_with("claude@"));
    }

    #[test]
    fn keychain_probe_covers_absolute_and_home_relative_config_paths() {
        let directory = tempdir().unwrap();
        let home = directory.path();
        let root = home.join(".claude-work");
        fs::create_dir_all(&root).unwrap();

        let literals = keychain_literals(home, &root);

        assert!(literals.contains(&root.to_string_lossy().into_owned()));
        assert!(literals.contains(&"~/.claude-work".to_owned()));
    }

    #[test]
    fn file_credentials_skip_non_macos_credential_store_discovery() {
        assert_eq!(
            should_probe_credential_store(true),
            cfg!(target_os = "macos")
        );
        assert!(should_probe_credential_store(false));
    }
}
