//! Creating, renaming and copying files and folders.
//!
//! Every path is placed by its parent: the parent must resolve inside the
//! canonical root and outside any `.git` folder, and the last component is
//! used as it is, never followed, so renaming or copying a symlink
//! acts on the link and not on its target. A path that must be new is
//! refused with `conflict` if anything, even a dangling link, has its name.
//!
//! The checks run just before the change. A parent that another process
//! swaps for a symlink in between is not detected, as elsewhere in this
//! crate; files are created with `create_new`, so at least the last
//! component is never followed or replaced.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::resolve;

fn io_error(path: &RelPath, err: &io::Error) -> ProtocolError {
    match err.kind() {
        io::ErrorKind::AlreadyExists => conflict(path),
        io::ErrorKind::Unsupported => {
            ProtocolError::new(ErrorCode::BadRequest, format!("{path}: {err}"))
        }
        _ => resolve::io_error(path, err),
    }
}

fn conflict(path: &RelPath) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, format!("{path} already exists"))
}

fn forbidden(path: &RelPath, why: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Forbidden, format!("{path} {why}"))
}

/// The parent of `path` and its last component.
fn split(path: &RelPath) -> Result<(RelPath, &str)> {
    let (parent, name) = path
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", path.as_str()));
    if name.is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "name a file or folder",
        ));
    }
    Ok((RelPath::parse(parent)?, name))
}

/// Refuses a resolved parent outside the root or inside a `.git` folder
/// (reached through a symlink: the core refuses `.git` in the path itself).
fn check_parent(root: &Path, real_parent: &Path, path: &RelPath) -> Result<()> {
    let root = resolve::root(root)?;
    let Ok(rest) = real_parent.strip_prefix(&root) else {
        return Err(forbidden(path, "points outside the project"));
    };
    let in_git = rest
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(".git")));
    if in_git {
        return Err(forbidden(path, "is inside .git"));
    }
    Ok(())
}

/// An existing `path`, by its resolved parent; the path itself is not
/// followed.
fn existing(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let (parent, name) = split(path)?;
    let (_, real_parent) = resolve::existing(root, &parent).map_err(|e| match e.code {
        ErrorCode::Forbidden => forbidden(path, "points outside the project"),
        _ => e,
    })?;
    check_parent(root, &real_parent, path)?;
    let full = real_parent.join(name);
    fs::symlink_metadata(&full).map_err(|e| io_error(path, &e))?;
    Ok(full)
}

/// Where a new `path` goes. Its parent's deepest existing ancestor resolves
/// inside the root, so the missing ones may be created there.
fn vacant(root: &Path, path: &RelPath) -> Result<PathBuf> {
    let (parent, name) = split(path)?;
    let dir = resolve::for_write(root, &parent).map_err(|e| match e.code {
        ErrorCode::Forbidden => forbidden(path, "points outside the project"),
        _ => e,
    })?;
    check_parent(root, &dir, path)?;
    let full = dir.join(name);
    match fs::symlink_metadata(&full) {
        Ok(_) => Err(conflict(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(full),
        Err(e) => Err(io_error(path, &e)),
    }
}

fn make_parents(target: &Path, path: &RelPath) -> Result<()> {
    match target.parent() {
        Some(dir) => fs::create_dir_all(dir).map_err(|e| io_error(path, &e)),
        None => Ok(()),
    }
}

/// Refuses `target` inside the folder `source`, also when a symlink in
/// `target`'s parents leads there.
fn not_inside(source: &Path, target: &Path) -> Result<()> {
    if target.starts_with(source) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "cannot put a folder inside itself",
        ));
    }
    Ok(())
}

pub(crate) fn create(root: &Path, path: &RelPath, dir: bool) -> Result<()> {
    let target = vacant(root, path)?;
    make_parents(&target, path)?;
    let made = if dir {
        fs::create_dir(&target)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map(drop)
    };
    made.map_err(|e| io_error(path, &e))
}

pub(crate) fn rename(root: &Path, from: &RelPath, to: &RelPath) -> Result<()> {
    let source = existing(root, from)?;
    let target = vacant(root, to)?;
    not_inside(&source, &target)?;
    make_parents(&target, to)?;
    // Check, then rename: there is no portable rename that refuses to
    // replace, so a file created at `to` since `vacant` would be replaced.
    fs::rename(&source, &target).map_err(|e| io_error(from, &e))
}

pub(crate) fn copy(root: &Path, from: &RelPath, to: &RelPath) -> Result<()> {
    let source = existing(root, from)?;
    let target = vacant(root, to)?;
    not_inside(&source, &target)?;
    make_parents(&target, to)?;
    // A failed copy leaves what was copied so far; nothing is deleted.
    copy_entry(&source, &target).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => conflict(to),
        _ => io_error(from, &e),
    })
}

/// Copies a file, a link (as a link) or a folder tree, never following links.
fn copy_entry(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        copy_link(from, to)
    } else if kind.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        // Last, so a read-only folder still gets its children.
        fs::set_permissions(to, meta.permissions())
    } else if kind.is_file() {
        let mut source = File::open(from)?;
        let mut target = OpenOptions::new().write(true).create_new(true).open(to)?;
        io::copy(&mut source, &mut target)?;
        target.set_permissions(meta.permissions())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{} is not a file, folder or link", from.display()),
        ))
    }
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

/// Windows needs to know whether a link is to a file or a folder, and a
/// privilege to make one; copying links is left out there.
#[cfg(not(unix))]
fn copy_link(from: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is a symlink, which Ovra cannot copy here",
            from.display()
        ),
    ))
}

#[cfg(test)]
mod tests;
