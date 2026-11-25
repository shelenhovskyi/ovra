//! The Landlock layer pod-init puts on every server it starts (Linux).
//! Only pod-init makes the sockets in the pod's folder: a server that made,
//! removed or renamed entries there could put its own socket, or a link,
//! where Ovra connects. Servers keep every other right the pod's sandbox
//! gives them, including writing files that are already in the folder.
//!
//! Landlock only grants, so the layer grants those rights below every
//! folder except the pod's: below each entry of each folder above it but
//! the one on the way down. That leaves one gap: entries right in those
//! folders above can be neither made nor removed by servers either, so
//! the pods' folder sits where servers do not write ([`crate::servers_dir`]),
//! and pod-init gives servers a `TMPDIR` of their own when theirs holds it.

use std::ffi::CStr;
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use landlock::{
    ABI, AccessFs, BitFlags, CompatLevel, Compatible as _, PathBeneath, Ruleset, RulesetAttr as _,
    RulesetCreated, RulesetCreatedAttr as _,
};
use rustix::fs::{Dir, FileType, Mode, OFlags};
use tokio::process::{Child, Command};
use tokio::runtime::Handle;

/// Starts `command` from a thread of its own under the layer for `pod`:
/// Landlock holds a thread and what it starts, so pod-init stays free.
pub(crate) fn spawn(pod: &Path, command: &mut Command) -> io::Result<Child> {
    let runtime = Handle::current();
    std::thread::scope(|scope| {
        let spawner = scope.spawn(|| {
            layer(pod)?.restrict_self().map_err(io::Error::other)?;
            let _entered = runtime.enter();
            command.spawn()
        });
        spawner
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the server's spawner failed")))
    })
}

/// Whether this kernel enforces the whole layer; without it a server may
/// make, remove and rename entries in its pod's folder (a reported gap).
pub(crate) fn available() -> bool {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(rights())
        .and_then(Ruleset::create)
        .is_ok()
}

/// Making, removing and renaming entries: what the layer handles.
fn rights() -> BitFlags<AccessFs> {
    let mut rights = AccessFs::from_write(ABI::V6);
    rights.remove(AccessFs::WriteFile | AccessFs::Truncate | AccessFs::IoctlDev);
    rights
}

fn layer(pod: &Path) -> io::Result<RulesetCreated> {
    let rights = rights();
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(rights)
        .and_then(Ruleset::create)
        .map_err(io::Error::other)?;
    let pod = std::fs::canonicalize(pod)?;
    let read = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut above = rustix::fs::open("/", read, Mode::empty())?;
    for part in pod.components() {
        let Component::Normal(next) = part else {
            continue;
        };
        for folder in folders_beside(&above, next.as_encoded_bytes())? {
            ruleset = ruleset
                .add_rule(PathBeneath::new(folder, rights))
                .map_err(io::Error::other)?;
        }
        above = rustix::fs::openat(&above, next, read, Mode::empty())?;
    }
    Ok(ruleset)
}

/// The folders in `dir` but `next`, never through a link.
fn folders_beside(dir: &OwnedFd, next: &[u8]) -> io::Result<Vec<OwnedFd>> {
    let mut folders = Vec::new();
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name: &CStr = entry.file_name();
        let skip = [&b"."[..], b"..", next].contains(&name.to_bytes());
        let maybe_folder = matches!(entry.file_type(), FileType::Directory | FileType::Unknown);
        if skip || !maybe_folder {
            continue;
        }
        let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        // Gone since, or not a folder after all: nothing to grant.
        if let Ok(folder) = rustix::fs::openat(dir, name, flags, Mode::empty()) {
            folders.push(folder);
        }
    }
    Ok(folders)
}
