use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::auth::{self, CodexAuthState};
use crate::{
    hashing::sha256_hex,
    storage::{Storage, StorageError},
};

#[derive(Debug, Clone)]
pub(super) struct CodexAccountDiscovery {
    pub default_account: Option<CodexAccount>,
    pub accounts: Vec<CodexAccount>,
}

#[derive(Debug, Clone)]
pub(super) struct CodexAccount {
    pub id: String,
    pub display_name: String,
    pub label: Option<String>,
    pub identity: String,
    pub auth_source: CodexAuthSource,
    pub session_roots: Vec<PathBuf>,
}

/// Describes where a Codex account's credentials are stored.
#[derive(Debug, Clone)]
pub(super) enum CodexAuthSource {
    /// Default credential locations (standard paths + macOS Keychain).
    /// Used when `CODEX_HOME` is not set or has a single value.
    Standard,
    /// A specific directory containing `auth.json`.
    /// Used when `CODEX_HOME` has multiple comma-separated values.
    Home(PathBuf),
    /// A specific directory containing Hermes `auth.json`, and the credential ID.
    Hermes(PathBuf, String),
}

#[derive(Debug, Clone)]
struct DiscoveredCodexAccount {
    identity: String,
    auth_source: CodexAuthSource,
    session_roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredCodexAccountPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

struct StoredAccountRecord {
    provider_id: String,
    label: Option<String>,
}

struct DiscoveredCodexAccounts {
    default_account: Option<DiscoveredCodexAccount>,
    accounts: Vec<DiscoveredCodexAccount>,
}

pub(super) fn discover(storage: &Storage) -> Result<CodexAccountDiscovery, StorageError> {
    let raw = discover_accounts();
    if raw.default_account.is_none() && raw.accounts.is_empty() {
        return Ok(CodexAccountDiscovery {
            default_account: None,
            accounts: Vec::new(),
        });
    }
    reconcile_accounts(storage, raw)
}

pub(super) fn identity_for_source(source: &CodexAuthSource) -> Option<String> {
    match source {
        CodexAuthSource::Standard => {
            CodexAuthState::observed_account_identity().map(|id| identity_stamp(&id))
        }
        CodexAuthSource::Home(path) => {
            let auth_path = path.join("auth.json");
            auth::load_from_path(&auth_path)
                .ok()?
                .account_identity()
                .map(|id| identity_stamp(&id))
        }
        CodexAuthSource::Hermes(path, hermes_id) => auth::load_hermes_from_path(path, hermes_id)
            .ok()?
            .account_identity()
            .map(|id| identity_stamp(&id)),
    }
}

fn discover_accounts() -> DiscoveredCodexAccounts {
    let home = home_directory();
    let configured_home = crate::provider_environment::value("CODEX_HOME");

    let multi_paths: Option<Vec<PathBuf>> = configured_home
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|configured| {
            configured
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| expand_home(v, &home))
                .collect()
        });

    if let Some(paths) = multi_paths {
        let result = discover_from_multiple_homes(&paths);
        if result.default_account.is_some() || !result.accounts.is_empty() {
            crate::app_info!(
                "config",
                "codex account discovery completed ({} account(s) from {} CODEX_HOME path(s))",
                1 + result.accounts.len(),
                paths.len()
            );
            return result;
        }
    }

    discover_from_standard()
}

fn discover_from_standard() -> DiscoveredCodexAccounts {
    let identity = CodexAuthState::observed_account_identity();
    let default_account = identity.map(|raw_identity| DiscoveredCodexAccount {
        identity: identity_stamp(&raw_identity),
        auth_source: CodexAuthSource::Standard,
        session_roots: Vec::new(),
    });
    DiscoveredCodexAccounts {
        default_account,
        accounts: Vec::new(),
    }
}

fn discover_from_multiple_homes(homes: &[PathBuf]) -> DiscoveredCodexAccounts {
    let mut seen_identities: BTreeMap<String, usize> = BTreeMap::new();
    let mut default_account = None;
    let mut extra_accounts = Vec::new();

    for (index, home_path) in homes.iter().enumerate() {
        let auth_path = home_path.join("auth.json");
        let discovered = auth::discover_identities_from_path(&auth_path);

        for (raw_identity, auth_source) in discovered {
            let identity = identity_stamp(&raw_identity);
            if seen_identities.contains_key(&identity) {
                continue;
            }
            seen_identities.insert(identity.clone(), index);

            let account = DiscoveredCodexAccount {
                identity,
                auth_source,
                session_roots: vec![home_path.clone()],
            };

            if default_account.is_none() {
                default_account = Some(account);
            } else {
                extra_accounts.push(account);
            }
        }
    }

    DiscoveredCodexAccounts {
        default_account,
        accounts: extra_accounts,
    }
}

fn reconcile_accounts(
    storage: &Storage,
    discovery: DiscoveredCodexAccounts,
) -> Result<CodexAccountDiscovery, StorageError> {
    let stored = storage.load_provider_account_records("codex")?;
    let records: BTreeMap<String, StoredAccountRecord> = stored
        .into_iter()
        .map(|(identity, provider_id, payload)| {
            let label = serde_json::from_str::<StoredCodexAccountPayload>(&payload)
                .ok()
                .and_then(|p| p.label);
            (identity, StoredAccountRecord { provider_id, label })
        })
        .collect();

    let mut occupied: HashSet<String> = records.values().map(|r| r.provider_id.clone()).collect();

    let has_bare_scoped_account = discovery.accounts.iter().any(|account| {
        records
            .get(&account.identity)
            .is_some_and(|r| r.provider_id == "codex")
    });

    let default_account = discovery
        .default_account
        .map(|account| {
            reconcile_account(
                storage,
                account,
                &records,
                &mut occupied,
                !has_bare_scoped_account,
            )
        })
        .transpose()?;

    let mut accounts: Vec<CodexAccount> = discovery
        .accounts
        .into_iter()
        .map(|account| reconcile_account(storage, account, &records, &mut occupied, false))
        .collect::<Result<Vec<_>, _>>()?;
    accounts.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(CodexAccountDiscovery {
        default_account,
        accounts,
    })
}

fn reconcile_account(
    storage: &Storage,
    account: DiscoveredCodexAccount,
    records: &BTreeMap<String, StoredAccountRecord>,
    occupied: &mut HashSet<String>,
    may_claim_default_id: bool,
) -> Result<CodexAccount, StorageError> {
    let record = records.get(&account.identity);
    let label = record.and_then(|r| r.label.clone());
    let id = record.map(|r| r.provider_id.clone()).unwrap_or_else(|| {
        if may_claim_default_id && !occupied.contains("codex") {
            "codex".to_owned()
        } else {
            allocate_account_id(&account.identity, occupied)
        }
    });
    occupied.insert(id.clone());

    let reconciled = CodexAccount {
        display_name: account_display_name_for_id(label.as_deref(), &id),
        id,
        label,
        identity: account.identity,
        auth_source: account.auth_source,
        session_roots: account.session_roots,
    };

    let payload = serde_json::to_string(&StoredCodexAccountPayload {
        label: reconciled.label.clone(),
    })?;
    storage.save_provider_account_record(
        "codex",
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
            sha256_hex(format!("{identity_stamp}:{salt}").as_bytes())
        };
        let candidate = format!("codex@{}", &stamp[..8]);
        if !occupied.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("an account ID is always available")
}

fn account_display_name_for_id(label: Option<&str>, id: &str) -> String {
    if id == "codex" {
        "Codex".to_owned()
    } else if let Some(label) = label.map(str::trim).filter(|v| !v.is_empty()) {
        format!("Codex — {label}")
    } else {
        id.to_owned()
    }
}

fn identity_stamp(identity: &str) -> String {
    sha256_hex(identity.to_ascii_lowercase().as_bytes())
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

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use tempfile::tempdir;

    use crate::storage::Storage;

    use super::{
        account_display_name_for_id, allocate_account_id, identity_stamp, reconcile_accounts,
        CodexAuthSource, DiscoveredCodexAccount, DiscoveredCodexAccounts,
    };

    #[test]
    fn default_account_claims_bare_id() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let discovery = DiscoveredCodexAccounts {
            default_account: Some(DiscoveredCodexAccount {
                identity: identity_stamp("account-a"),
                auth_source: CodexAuthSource::Standard,
                session_roots: Vec::new(),
            }),
            accounts: Vec::new(),
        };

        let result = reconcile_accounts(&storage, discovery).unwrap();

        assert_eq!(result.default_account.as_ref().unwrap().id, "codex");
        assert!(result.accounts.is_empty());
    }

    #[test]
    fn extra_accounts_receive_hashed_ids() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let discovery = DiscoveredCodexAccounts {
            default_account: Some(DiscoveredCodexAccount {
                identity: identity_stamp("account-a"),
                auth_source: CodexAuthSource::Standard,
                session_roots: Vec::new(),
            }),
            accounts: vec![DiscoveredCodexAccount {
                identity: identity_stamp("account-b"),
                auth_source: CodexAuthSource::Home("/tmp/codex-work".into()),
                session_roots: vec!["/tmp/codex-work".into()],
            }],
        };

        let result = reconcile_accounts(&storage, discovery).unwrap();

        assert_eq!(result.default_account.as_ref().unwrap().id, "codex");
        assert_eq!(result.accounts.len(), 1);
        assert!(result.accounts[0].id.starts_with("codex@"));
        assert_eq!(result.accounts[0].id.len(), "codex@".len() + 8);
    }

    #[test]
    fn stable_ids_across_restarts() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("openquota.db")).unwrap();
        let identity_b = identity_stamp("account-b");

        // First discovery
        let discovery = DiscoveredCodexAccounts {
            default_account: Some(DiscoveredCodexAccount {
                identity: identity_stamp("account-a"),
                auth_source: CodexAuthSource::Standard,
                session_roots: Vec::new(),
            }),
            accounts: vec![DiscoveredCodexAccount {
                identity: identity_b.clone(),
                auth_source: CodexAuthSource::Home("/tmp/codex-work".into()),
                session_roots: vec!["/tmp/codex-work".into()],
            }],
        };
        let first = reconcile_accounts(&storage, discovery).unwrap();
        let first_extra_id = first.accounts[0].id.clone();

        // Second discovery with same accounts
        let discovery2 = DiscoveredCodexAccounts {
            default_account: Some(DiscoveredCodexAccount {
                identity: identity_stamp("account-a"),
                auth_source: CodexAuthSource::Standard,
                session_roots: Vec::new(),
            }),
            accounts: vec![DiscoveredCodexAccount {
                identity: identity_b,
                auth_source: CodexAuthSource::Home("/tmp/codex-work".into()),
                session_roots: vec!["/tmp/codex-work".into()],
            }],
        };
        let second = reconcile_accounts(&storage, discovery2).unwrap();

        assert_eq!(
            first.default_account.unwrap().id,
            second.default_account.unwrap().id
        );
        assert_eq!(first_extra_id, second.accounts[0].id);
    }

    #[test]
    fn allocate_avoids_collisions() {
        let mut occupied = std::collections::HashSet::new();
        let stamp = identity_stamp("test");
        let first = allocate_account_id(&stamp, &occupied);
        occupied.insert(first.clone());
        let second = allocate_account_id(&stamp, &occupied);

        assert_ne!(first, second);
        assert!(first.starts_with("codex@"));
        assert!(second.starts_with("codex@"));
    }

    #[test]
    fn display_name_for_default_and_extra_accounts() {
        assert_eq!(account_display_name_for_id(None, "codex"), "Codex");
        assert_eq!(
            account_display_name_for_id(Some("Work"), "codex@12345678"),
            "Codex — Work"
        );
        assert_eq!(
            account_display_name_for_id(None, "codex@12345678"),
            "codex@12345678"
        );
    }
}
