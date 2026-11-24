//! Secrets in the OS keychain (`docs/specs/secrets.md`): values only in
//! the keychain, names and bindings in `secrets.json` in Ovra's data
//! folder. Contract: `ovra_core::services::Secrets`; fake:
//! `ovra_core::fakes::MemorySecrets`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use keyring_core::{CredentialStore, Entry};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::secrets::{SecretInfo, SecretStoreStatus, SecretValue};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{Result, Secrets};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// The keychain item's service; the account names install, project,
/// secret and generation.
const SERVICE: &str = "ovra";
const FILE: &str = "secrets.json";

type Opener = fn() -> keyring_core::Result<Arc<CredentialStore>>;

/// [`Secrets`] over this OS's keychain. The keychain is opened on first
/// use, so a machine without one starts fine and says why on `status`.
pub struct KeychainSecrets {
    file: PathBuf,
    open: Opener,
    store: OnceLock<std::result::Result<Arc<CredentialStore>, String>>,
    /// Held across a whole `set` or `remove`.
    lock: Mutex<()>,
}

impl std::fmt::Debug for KeychainSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeychainSecrets")
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// What `secrets.json` holds: no values.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// Random per data folder, so two folders never share an item.
    install: String,
    #[serde(default)]
    projects: BTreeMap<String, Vec<Stored>>,
    /// Keychain accounts to delete; retried before each change.
    #[serde(default)]
    pending_delete: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    info: SecretInfo,
    /// Random per write, so a new value never sits under an old binding.
    generation: String,
}

impl KeychainSecrets {
    /// Secrets for the data folder `data_dir`, in this OS's keychain.
    pub fn new(data_dir: &Path) -> Self {
        Self::with_opener(data_dir, platform_store)
    }

    fn with_opener(data_dir: &Path, open: Opener) -> Self {
        Self {
            file: data_dir.join(FILE),
            open,
            store: OnceLock::new(),
            lock: Mutex::new(()),
        }
    }

    fn store(&self) -> Result<&Arc<CredentialStore>> {
        self.store
            .get_or_init(|| (self.open)().map_err(|e| describe(&e)))
            .as_ref()
            .map_err(|reason| ProtocolError::new(ErrorCode::Unavailable, unreachable(reason)))
    }

    fn entry(&self, account: &str) -> Result<Entry> {
        Entry::new_in_store(self.store()?, SERVICE, account).map_err(|e| keychain_error(&e))
    }

    fn load(&self) -> Result<Option<Index>> {
        match std::fs::read(&self.file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    format!(
                        "{} cannot be read; Ovra leaves it as it is",
                        self.file.display()
                    ),
                )
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("{} cannot be read: {e}", self.file.display()),
            )),
        }
    }

    fn load_or_new(&self) -> Result<Index> {
        Ok(self.load()?.unwrap_or_else(|| Index {
            install: random_id(),
            ..Index::default()
        }))
    }

    /// Writes the index atomically: a private temporary file, synced, then
    /// renamed over the old one.
    fn save(&self, index: &Index) -> Result<()> {
        let fail = |e: std::io::Error| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("{} cannot be written: {e}", self.file.display()),
            )
        };
        let bytes = serde_json::to_vec_pretty(index)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?;
        let tmp = self.file.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp).map_err(fail)?;
        file.write_all(&bytes).map_err(fail)?;
        file.sync_all().map_err(fail)?;
        drop(file);
        std::fs::rename(&tmp, &self.file).map_err(fail)
    }

    /// Deletes the pending items it can; the rest stay pending.
    fn flush(&self, index: &mut Index) {
        index.pending_delete.retain(|account| {
            let gone = self
                .entry(account)
                .map(|entry| match entry.delete_credential() {
                    Ok(()) | Err(keyring_core::Error::NoEntry) => true,
                    Err(_) => false,
                })
                .unwrap_or(false);
            !gone
        });
    }

    /// Saves, deletes what is pending, and saves again if that changed
    /// anything.
    fn commit(&self, index: &mut Index) -> Result<()> {
        self.save(index)?;
        let before = index.pending_delete.len();
        self.flush(index);
        if index.pending_delete.len() != before {
            // The change is saved; a failure here only leaves deletes
            // pending, and deleting a missing item is fine.
            let _ = self.save(index);
        }
        Ok(())
    }
}

impl Secrets for KeychainSecrets {
    fn status(&self) -> SecretStoreStatus {
        match self.store() {
            Ok(_) => SecretStoreStatus::Available {
                name: STORE_NAME.into(),
            },
            Err(e) => SecretStoreStatus::Unavailable { reason: e.message },
        }
    }

    fn list(&self, project: &ProjectId) -> Result<Vec<SecretInfo>> {
        let index = self.load()?.unwrap_or_default();
        let mut list: Vec<SecretInfo> = index
            .projects
            .get(&project.0)
            .into_iter()
            .flatten()
            .map(|s| s.info.clone())
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn set(&self, project: &ProjectId, secret: SecretInfo, value: SecretValue) -> Result<()> {
        let _held = lock(&self.lock);
        let mut index = self.load_or_new()?;
        let stored = Stored {
            info: secret,
            generation: random_id(),
        };
        let new = account(&index.install, project, &stored);
        self.entry(&new)?
            .set_password(value.expose())
            .map_err(|e| keychain_error(&e))?;
        let list = index.projects.entry(project.0.clone()).or_default();
        let old = list
            .iter()
            .position(|s| s.info.name == stored.info.name)
            .map(|at| list.remove(at));
        list.push(stored);
        if let Some(old) = old {
            let old = account(&index.install, project, &old);
            index.pending_delete.push(old);
        }
        if let Err(e) = self.commit(&mut index) {
            // Not listed, so never sent: take the new item back if we can.
            if let Ok(entry) = self.entry(&new) {
                let _ = entry.delete_credential();
            }
            return Err(e);
        }
        Ok(())
    }

    fn remove(&self, project: &ProjectId, name: &str) -> Result<()> {
        let _held = lock(&self.lock);
        let mut index = self.load()?.ok_or_else(|| no_secret(name))?;
        let list = index.projects.entry(project.0.clone()).or_default();
        let at = list
            .iter()
            .position(|s| s.info.name == name)
            .ok_or_else(|| no_secret(name))?;
        let old = list.remove(at);
        if list.is_empty() {
            index.projects.remove(&project.0);
        }
        let old = account(&index.install, project, &old);
        index.pending_delete.push(old);
        self.commit(&mut index)
    }

    fn remove_project(&self, project: &ProjectId) -> Result<()> {
        let _held = lock(&self.lock);
        let Some(mut index) = self.load()? else {
            return Ok(());
        };
        let Some(list) = index.projects.remove(&project.0) else {
            return Ok(());
        };
        for old in &list {
            let old = account(&index.install, project, old);
            index.pending_delete.push(old);
        }
        self.commit(&mut index)
    }

    fn value(&self, project: &ProjectId, name: &str) -> Result<SecretValue> {
        let index = self.load()?.ok_or_else(|| no_secret(name))?;
        let stored = index
            .projects
            .get(&project.0)
            .and_then(|list| list.iter().find(|s| s.info.name == name))
            .ok_or_else(|| no_secret(name))?;
        let account = account(&index.install, project, stored);
        match self.entry(&account)?.get_password() {
            Ok(value) => Ok(SecretValue::new(value)),
            Err(keyring_core::Error::NoEntry) => Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("{name} is listed but missing from the keychain; set it again"),
            )),
            Err(e) => Err(keychain_error(&e)),
        }
    }
}

fn account(install: &str, project: &ProjectId, stored: &Stored) -> String {
    format!(
        "{install}/{}/{}/{}",
        project.0, stored.info.name, stored.generation
    )
}

fn random_id() -> String {
    let bytes: [u8; 8] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn no_secret(name: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("no secret named {name}"))
}

fn unreachable(reason: &str) -> String {
    format!("the {STORE_NAME} cannot be reached ({reason}); Ovra keeps secrets nowhere else")
}

/// A keychain error as text. Variants that carry stored bytes are never
/// printed, because those bytes may be a value.
fn describe(e: &keyring_core::Error) -> String {
    use keyring_core::Error as E;
    match e {
        E::PlatformFailure(p) | E::NoStorageAccess(p) => p.to_string(),
        E::NoEntry => "no such item".into(),
        E::BadEncoding(_) | E::BadDataFormat(..) => "an item holds data Ovra cannot read".into(),
        E::TooLong(what, max) => format!("{what} is longer than {max}"),
        E::NoDefaultStore => "no keychain is set up".into(),
        E::NotSupportedByStore(what) => format!("not supported: {what}"),
        _ => "the keychain refused the call".into(),
    }
}

fn keychain_error(e: &keyring_core::Error) -> ProtocolError {
    match e {
        keyring_core::Error::NoStorageAccess(_) => {
            ProtocolError::new(ErrorCode::Unavailable, unreachable(&describe(e)))
        }
        _ => ProtocolError::new(
            ErrorCode::Internal,
            format!("the {STORE_NAME} failed: {}", describe(e)),
        ),
    }
}

#[cfg(target_os = "macos")]
const STORE_NAME: &str = "macOS Keychain";
#[cfg(windows)]
const STORE_NAME: &str = "Windows Credential Manager";
#[cfg(all(unix, not(target_os = "macos")))]
const STORE_NAME: &str = "Secret Service";

#[cfg(target_os = "macos")]
fn platform_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(windows)]
fn platform_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(test)]
mod tests;
