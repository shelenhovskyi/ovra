//! The protected folders sessions made in one work dir, and when they go
//! away again.
//!
//! Linux lets a process remove an empty folder that is a mount point in
//! another mount namespace, and detaches the bind there. So a folder made
//! for one sandbox and bound read-only in every sandbox of that work dir
//! is removed only after the last of them has ended: each holds a shared
//! `flock` on the work dir's record (in Ovra's data folder, hidden from
//! every sandbox) while it runs, and whoever gets it exclusively once its
//! own sandbox has ended removes the folders the record lists.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

/// The record of the work dir named `key`, under Ovra's data folder.
pub(crate) fn record_of(data_dir: &Path, key: &str) -> PathBuf {
    data_dir.join("sandbox-made").join(key)
}

/// Opens the record, making it if missing, and holds it shared.
pub(crate) fn hold(record: &Path) -> std::io::Result<File> {
    if let Some(parent) = record.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(record)?;
    file.lock_shared()?;
    Ok(file)
}

/// Adds `folders` to the record `held` (from [`hold`]).
pub(crate) fn add(mut held: &File, folders: &[PathBuf]) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    for folder in folders {
        bytes.extend_from_slice(folder.as_os_str().as_bytes());
        bytes.push(0);
    }
    held.seek(std::io::SeekFrom::End(0))?;
    held.write_all(&bytes)
}

/// Once the sandbox that held `held` has ended: if no other sandbox of the
/// work dir holds the record, removes the folders it lists (only empty
/// ones, never through a link) and empties it. Otherwise the last one
/// does.
pub(crate) fn clean(mut held: File) -> std::io::Result<()> {
    // Turns the shared lock into an exclusive one, or gives it up.
    if held.try_lock().is_err() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    held.rewind()?;
    held.read_to_end(&mut bytes)?;
    for folder in bytes.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        let _ = fs::remove_dir(OsStr::from_bytes(folder));
    }
    held.set_len(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_folders_go_only_when_the_last_holder_cleans() {
        let root = std::env::temp_dir().join(format!("ovra-made-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (vscode, idea) = (root.join("work/.vscode"), root.join("work/.idea"));
        fs::create_dir_all(&vscode).unwrap();
        fs::create_dir_all(&idea).unwrap();
        let record = record_of(&root.join("data"), "work");
        let first = hold(&record).unwrap();
        add(&first, std::slice::from_ref(&vscode)).unwrap();
        let second = hold(&record).unwrap();
        add(&second, std::slice::from_ref(&idea)).unwrap();
        clean(first).unwrap();
        assert!(vscode.is_dir() && idea.is_dir());
        clean(second).unwrap();
        assert!(!vscode.exists() && !idea.exists());
        assert_eq!(fs::read(&record).unwrap(), b"");
        fs::remove_dir_all(&root).unwrap();
    }
}
