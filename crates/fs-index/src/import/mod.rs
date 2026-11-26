//! Copying what the person dropped onto the window from the system's file
//! manager into a project folder.
//!
//! The sources are absolute paths the desktop app took from the window's
//! drop event and keeps under a drop id (`Drops`); they never come from a
//! client frame, so neither headless mode nor agents can reach this. The
//! target folder is checked like `fs.copy`'s: it resolves inside the root
//! and outside `.git`. Everything is looked at first, without following
//! links: links are skipped (and named in the result), a device, pipe or
//! socket refuses the whole drop, and so does more than `Limits` allows.
//! An existing name is never replaced without an answer (`Clash`); a
//! replaced entry goes to the trash first. Files and folders are then
//! copied like `edit::copy` copies them, with the permission bits only.

pub(crate) mod drops;

use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::edit::{self, Trash};
use crate::resolve;

/// What to do when a dropped name already exists in the target folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    /// Copy nothing and report the names.
    Ask,
    /// Move the existing entries to the trash, then copy.
    Replace,
    /// Copy under a free name: `a copy.png`, `a copy 2.png`, ...
    KeepBoth,
}

/// The result of an import.
#[derive(Debug, PartialEq, Eq)]
pub enum Imported {
    /// Nothing was copied: these dropped names already exist (`Clash::Ask`).
    Clashes(Vec<String>),
    /// The new top-level entries, and the links that were left out, as
    /// paths starting with the dropped name.
    Copied {
        paths: Vec<RelPath>,
        skipped_links: Vec<String>,
    },
}

/// The most one drop may copy.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) entries: u64,
    pub(crate) bytes: u64,
}

pub(crate) const LIMITS: Limits = Limits {
    entries: 10_000,
    bytes: 1 << 30,
};

struct Item<'a> {
    source: &'a Path,
    name: String,
    /// The source with its parent resolved; the source itself is not followed.
    real: PathBuf,
}

pub(crate) fn import(
    root: &Path,
    sources: &[PathBuf],
    into: &RelPath,
    clash: Clash,
    trash: &Trash,
    limits: Limits,
) -> Result<Imported> {
    let dir = target_dir(root, into)?;
    let mut scan = Walk::new(limits);
    let mut items: Vec<Item<'_>> = Vec::new();
    for source in sources {
        let name = source_name(source, into)?;
        if items.iter().any(|i| i.name.eq_ignore_ascii_case(&name)) {
            return Err(bad(format!("Two dropped items are named {name}.")));
        }
        let meta = fs::symlink_metadata(source).map_err(|e| resolve::io_error(&name, &e))?;
        if meta.file_type().is_symlink() {
            scan.links.push(name);
            continue;
        }
        let real = real_source(source, &name)?;
        edit::not_inside(&real, &dir)?;
        scan.visit(source, None, &name)?;
        items.push(Item { source, name, real });
    }
    let clashes: Vec<String> = items
        .iter()
        .filter(|i| present(&dir.join(&i.name)))
        .map(|i| i.name.clone())
        .collect();
    if !clashes.is_empty() && clash == Clash::Ask {
        return Ok(Imported::Clashes(clashes));
    }
    let mut copy = Walk::new(limits);
    let mut paths = Vec::new();
    for item in &items {
        let old = dir.join(&item.name);
        let mut name = item.name.clone();
        if present(&old) {
            if clash == Clash::KeepBoth {
                name = copy_name(&item.name, item.real.is_dir(), &dir);
            } else if item.real.starts_with(&old) {
                return Err(bad(format!(
                    "{} cannot replace itself or a folder it is in.",
                    item.name
                )));
            } else {
                edit::trash(root, &[into.join(&item.name)?], trash)?;
            }
        }
        copy.visit(item.source, Some(&dir.join(&name)), &item.name)?;
        paths.push(into.join(&name)?);
    }
    Ok(Imported::Copied {
        paths,
        skipped_links: scan.links,
    })
}

fn bad(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// The existing folder `into`, resolved inside the root and outside `.git`.
fn target_dir(root: &Path, into: &RelPath) -> Result<PathBuf> {
    let (_, real) = resolve::existing(root, into)?;
    if !real.is_dir() {
        return Err(bad(format!("{into} is not a folder.")));
    }
    edit::check_parent(root, &real, into)?;
    Ok(real)
}

/// The name a dropped `source` gets in `into`, refused unless it is a name
/// the project may hold.
fn source_name(source: &Path, into: &RelPath) -> Result<String> {
    if !source.is_absolute() {
        return Err(bad(format!("{} is not a full path.", source.display())));
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| bad(format!("{} has no name Ovra can use.", source.display())))?;
    if name.eq_ignore_ascii_case(".git") {
        return Err(bad("A .git folder cannot be dropped into a project.".into()));
    }
    into.join(name)?;
    Ok(name.to_owned())
}

fn real_source(source: &Path, name: &str) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or(source);
    let parent = fs::canonicalize(parent).map_err(|e| resolve::io_error(name, &e))?;
    Ok(parent.join(name))
}

fn present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// A free name for a copy of `name` in `dir`, as the tree's Duplicate names
/// one (`copyName` in `ui/src/tree/ops.ts`).
fn copy_name(name: &str, is_dir: bool, dir: &Path) -> String {
    let end = match name.rfind('.') {
        Some(dot) if dot > 0 && !is_dir => dot,
        _ => name.len(),
    };
    let (stem, ext) = name.split_at(end);
    (1..)
        .map(|n| match n {
            1 => format!("{stem} copy{ext}"),
            n => format!("{stem} copy {n}{ext}"),
        })
        .find(|candidate| !present(&dir.join(candidate)))
        .unwrap_or_default()
}

/// Walks a dropped tree without following links, counting against the
/// limits, and copies it when given a target.
struct Walk {
    limits: Limits,
    entries: u64,
    bytes: u64,
    links: Vec<String>,
}

impl Walk {
    fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: 0,
            bytes: 0,
            links: Vec::new(),
        }
    }

    fn add_bytes(&mut self, bytes: u64) -> Result<()> {
        self.bytes += bytes;
        if self.bytes > self.limits.bytes {
            return Err(bad(format!(
                "That is more than {} MB; Ovra copies at most that much in one drop.",
                self.limits.bytes >> 20
            )));
        }
        Ok(())
    }

    fn visit(&mut self, from: &Path, to: Option<&Path>, shown: &str) -> Result<()> {
        let fail = |e: io::Error| match e.kind() {
            io::ErrorKind::AlreadyExists => {
                ProtocolError::new(ErrorCode::Conflict, format!("{shown} already exists"))
            }
            _ => resolve::io_error(shown, &e),
        };
        let meta = fs::symlink_metadata(from).map_err(fail)?;
        let kind = meta.file_type();
        if kind.is_symlink() {
            self.links.push(shown.to_owned());
            return Ok(());
        }
        self.entries += 1;
        if self.entries > self.limits.entries {
            return Err(bad(format!(
                "That is more than {} items; Ovra copies at most that many in one drop.",
                self.limits.entries
            )));
        }
        if kind.is_dir() {
            edit::check_dir(from, &meta).map_err(fail)?;
            if let Some(to) = to {
                fs::create_dir(to).map_err(fail)?;
            }
            for entry in fs::read_dir(from).map_err(fail)? {
                let entry = entry.map_err(fail)?;
                let name = entry.file_name();
                let shown = format!("{shown}/{}", name.to_string_lossy());
                let to = to.map(|to| to.join(&name));
                self.visit(&entry.path(), to.as_deref(), &shown)?;
            }
            // Last, so a read-only folder still gets its children.
            if let Some(to) = to {
                fs::set_permissions(to, permissions(&meta)).map_err(fail)?;
            }
            Ok(())
        } else if kind.is_file() {
            let Some(to) = to else {
                return self.add_bytes(meta.len());
            };
            let source = edit::open_file(from, &meta).map_err(fail)?;
            let mut target = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(to)
                .map_err(fail)?;
            // A file that grew since the scan stops at the limit.
            let left = self.limits.bytes.saturating_sub(self.bytes);
            let copied = io::copy(&mut source.take(left + 1), &mut target).map_err(fail)?;
            self.add_bytes(copied)?;
            target.set_permissions(permissions(&meta)).map_err(fail)
        } else {
            Err(bad(format!(
                "{shown} is not a file or folder (a device, pipe or socket), so Ovra does not copy it."
            )))
        }
    }
}

/// The source's read, write and execute bits; never setuid, setgid or sticky.
#[cfg(unix)]
fn permissions(meta: &fs::Metadata) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn permissions(meta: &fs::Metadata) -> fs::Permissions {
    meta.permissions()
}

#[cfg(test)]
mod tests;
