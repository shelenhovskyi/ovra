use std::sync::Arc;

use keyring_core::mock;
use ovra_core::protocol::ErrorCode;
use ovra_core::protocol::projects::ProjectId;
use ovra_core::protocol::secrets::{SecretInfo, SecretStoreStatus, SecretValue};
use ovra_core::services::Secrets;

use super::{FILE, KeychainSecrets};

thread_local! {
    /// One mock keychain per test thread, shared by every `KeychainSecrets`
    /// the test opens (as the real keychain would be).
    static KEYCHAIN: Arc<keyring_core::CredentialStore> = mock::Store::new().unwrap_or_else(|e| panic!("{e}"));
}

fn mock_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(KEYCHAIN.with(Arc::clone))
}

fn no_keychain() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Err(keyring_core::Error::NoDefaultStore)
}

fn info(name: &str, host: &str) -> SecretInfo {
    SecretInfo {
        name: name.into(),
        host: host.into(),
        header: "Authorization".into(),
        prefix: "Bearer ".into(),
    }
}

fn p(id: &str) -> ProjectId {
    ProjectId(id.into())
}

/// How many items the mock keychain holds with a value.
fn items(secrets: &KeychainSecrets, accounts: &[String]) -> usize {
    accounts
        .iter()
        .filter(|a| secrets.entry(a).is_ok_and(|e| e.get_password().is_ok()))
        .count()
}

fn value(secrets: &KeychainSecrets, project: &str, name: &str) -> Option<String> {
    secrets
        .value(&p(project), name)
        .ok()
        .map(|v| v.expose().to_owned())
}

#[test]
fn values_live_in_the_keychain_and_only_bindings_in_the_file() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let secrets = KeychainSecrets::with_opener(dir.path(), mock_store);
    assert!(matches!(
        secrets.status(),
        SecretStoreStatus::Available { .. }
    ));
    assert_eq!(secrets.list(&p("p1")).ok(), Some(Vec::new()));

    let set = secrets.set(
        &p("p1"),
        info("gh", "api.github.com"),
        SecretValue::new("ghp_one"),
    );
    assert!(set.is_ok(), "{set:?}");
    assert!(
        secrets
            .set(&p("p1"), info("a", "x.example.com"), SecretValue::new("a1"))
            .is_ok()
    );
    assert!(
        secrets
            .set(
                &p("p2"),
                info("gh", "api.github.com"),
                SecretValue::new("ghp_two")
            )
            .is_ok()
    );

    let names: Vec<String> = secrets
        .list(&p("p1"))
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["a", "gh"]);
    assert_eq!(value(&secrets, "p1", "gh").as_deref(), Some("ghp_one"));
    assert_eq!(value(&secrets, "p2", "gh").as_deref(), Some("ghp_two"));

    let file = std::fs::read_to_string(dir.path().join(FILE)).unwrap_or_default();
    assert!(file.contains("api.github.com"), "{file}");
    assert!(!file.contains("ghp_"), "no value in the file: {file}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(FILE)).map(|m| m.permissions().mode());
        assert_eq!(mode.ok().map(|m| m & 0o777), Some(0o600));
    }

    // Another instance on the same folder (a restart) sees the same.
    let again = KeychainSecrets::with_opener(dir.path(), mock_store);
    assert_eq!(value(&again, "p1", "gh").as_deref(), Some("ghp_one"));
}

#[test]
fn replacing_or_removing_deletes_the_old_item() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let secrets = KeychainSecrets::with_opener(dir.path(), mock_store);
    let accounts = || {
        let index = secrets.load().ok().flatten().unwrap_or_default();
        let mut all: Vec<String> = index
            .projects
            .iter()
            .flat_map(|(project, list)| {
                list.iter()
                    .map(|s| super::account(&index.install, &p(project), s))
                    .collect::<Vec<_>>()
            })
            .collect();
        all.extend(index.pending_delete);
        all
    };
    assert!(
        secrets
            .set(
                &p("p1"),
                info("gh", "api.github.com"),
                SecretValue::new("one")
            )
            .is_ok()
    );
    let first = accounts();
    // A new host gets a new item; the old one is deleted, not overwritten.
    assert!(
        secrets
            .set(
                &p("p1"),
                info("gh", "uploads.github.com"),
                SecretValue::new("two")
            )
            .is_ok()
    );
    let second = accounts();
    assert_ne!(first, second);
    assert_eq!(items(&secrets, &first), 0);
    assert_eq!(items(&secrets, &second), 1);
    assert_eq!(value(&secrets, "p1", "gh").as_deref(), Some("two"));

    assert!(secrets.remove(&p("p1"), "gh").is_ok());
    assert_eq!(items(&secrets, &second), 0);
    assert_eq!(
        secrets.remove(&p("p1"), "gh").err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
    assert_eq!(
        secrets.value(&p("p1"), "gh").err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );

    assert!(
        secrets
            .set(&p("p3"), info("a", "a.example.com"), SecretValue::new("1"))
            .is_ok()
    );
    assert!(
        secrets
            .set(&p("p3"), info("b", "b.example.com"), SecretValue::new("2"))
            .is_ok()
    );
    let third = accounts();
    assert!(secrets.remove_project(&p("p3")).is_ok());
    assert_eq!(items(&secrets, &third), 0);
    assert_eq!(secrets.list(&p("p3")).ok(), Some(Vec::new()));
    assert!(secrets.remove_project(&p("p9")).is_ok());
}

#[test]
fn a_failed_keychain_delete_stays_pending_and_is_retried() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let secrets = KeychainSecrets::with_opener(dir.path(), mock_store);
    assert!(
        secrets
            .set(
                &p("p1"),
                info("gh", "api.github.com"),
                SecretValue::new("one")
            )
            .is_ok()
    );
    let index = secrets.load().ok().flatten().unwrap_or_default();
    let stored = &index.projects["p1"][0];
    let account = super::account(&index.install, &p("p1"), stored);
    let entry = secrets.entry(&account).unwrap_or_else(|e| panic!("{e}"));
    let cred: Option<&mock::Cred> = entry.as_any().downcast_ref();
    let fail = || keyring_core::Error::NoStorageAccess(Box::new(std::io::Error::other("locked")));
    cred.unwrap_or_else(|| panic!("not a mock"))
        .set_error(fail());

    // The name is gone at once; the item waits for the next change.
    assert!(secrets.remove(&p("p1"), "gh").is_ok());
    assert_eq!(secrets.list(&p("p1")).ok(), Some(Vec::new()));
    let pending = secrets
        .load()
        .ok()
        .flatten()
        .unwrap_or_default()
        .pending_delete;
    assert_eq!(pending, std::slice::from_ref(&account));
    assert_eq!(items(&secrets, std::slice::from_ref(&account)), 1);

    assert!(
        secrets
            .set(&p("p1"), info("b", "b.example.com"), SecretValue::new("2"))
            .is_ok()
    );
    let pending = secrets
        .load()
        .ok()
        .flatten()
        .unwrap_or_default()
        .pending_delete;
    assert!(pending.is_empty(), "{pending:?}");
    assert_eq!(items(&secrets, &[account]), 0);
}

#[test]
fn without_a_keychain_nothing_is_written_and_status_says_why() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let secrets = KeychainSecrets::with_opener(dir.path(), no_keychain);
    let SecretStoreStatus::Unavailable { reason } = secrets.status() else {
        panic!("available without a keychain");
    };
    assert!(reason.contains("nowhere else"), "{reason}");
    let set = secrets.set(
        &p("p1"),
        info("gh", "api.github.com"),
        SecretValue::new("ghp_x"),
    );
    assert_eq!(set.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    assert!(
        !dir.path().join(FILE).exists(),
        "no file without a keychain"
    );
    assert_eq!(secrets.list(&p("p1")).ok(), Some(Vec::new()));
}

#[test]
fn an_unreadable_list_is_left_alone() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = dir.path().join(FILE);
    assert!(std::fs::write(&path, "{not json").is_ok());
    let secrets = KeychainSecrets::with_opener(dir.path(), mock_store);
    let set = secrets.set(
        &p("p1"),
        info("gh", "api.github.com"),
        SecretValue::new("v"),
    );
    assert_eq!(set.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    assert_eq!(
        secrets.list(&p("p1")).err().map(|e| e.code),
        Some(ErrorCode::Unavailable)
    );
    assert_eq!(
        std::fs::read_to_string(&path).ok().as_deref(),
        Some("{not json")
    );
}

thread_local! {
    static KEYCHAIN_UP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn later_keychain() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    if KEYCHAIN_UP.with(std::cell::Cell::get) {
        mock_store()
    } else {
        Err(keyring_core::Error::NoDefaultStore)
    }
}

#[test]
fn a_keychain_that_starts_later_is_used_once_it_is_up() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let secrets = KeychainSecrets::with_opener(dir.path(), later_keychain);
    assert!(matches!(
        secrets.status(),
        SecretStoreStatus::Unavailable { .. }
    ));
    KEYCHAIN_UP.with(|up| up.set(true));
    assert!(matches!(
        secrets.status(),
        SecretStoreStatus::Available { .. }
    ));
    assert!(
        secrets
            .set(
                &p("p1"),
                info("gh", "api.github.com"),
                SecretValue::new("v")
            )
            .is_ok()
    );
}
