//! Copying what the person dropped onto the window from the system's file
//! manager into a project folder.
//!
//! The sources are absolute paths the desktop app took from the window's
//! drop event and keeps under a drop id (`Drops`); they never come from a
//! client frame, so neither headless mode nor agents can reach this. The
//! target folder is checked like `fs.copy`'s: it resolves inside the root
//! and outside `.git`; it must not be reached through a link either. As an
//! agent may change the project while the person's import runs, the walk
//! goes by handle (`node`), not by paths it resolves again.
//!
//! An import is planned first, without following links and before anything
//! changes: links are skipped (and named in the result), a device, pipe or
//! socket refuses the whole drop, and so does more than `Limits` allows.
//! A name that exists already is only replaced when the person was shown it
//! (`Imported::Clashes`) and answered `Clash::Replace`; every refusal comes
//! before the first change. Then each replaced entry goes to the trash and
//! the sources are copied like `edit::copy` copies them, never over
//! anything: a name that appears meanwhile stops the import with
//! `conflict`. A failed import keeps what was copied before the failure,
//! but not a partly copied file.

pub(crate) mod drops;
mod node;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use unicode_normalization::UnicodeNormalization;

use self::drops::Drops;
use self::node::Kind;
use crate::edit::{self, Trash};
use crate::resolve;

/// What to do when a dropped name already exists in the target folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    /// Copy nothing and report the names.
    Ask,
    /// Move the existing entries the person was shown to the trash, then
    /// copy.
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
    /// Folders inside folders, below the dropped one.
    pub(crate) depth: usize,
}

pub(crate) const LIMITS: Limits = Limits {
    entries: 10_000,
    bytes: 1 << 30,
    depth: 256,
};

/// Where and how to import a drop.
pub(crate) struct Target<'a> {
    pub(crate) root: &'a Path,
    pub(crate) into: &'a RelPath,
    pub(crate) clash: Clash,
}

/// Takes the drop `id` and imports it (`import`). A drop that only
/// reported clashes is put back with the names it showed, for the answer;
/// any other outcome uses it up.
pub(crate) fn import_drop(
    drops: &Drops,
    id: &str,
    now: Instant,
    to: &Target<'_>,
    trash: &Trash,
    limits: Limits,
) -> Result<Imported> {
    let mut dropped = drops.take(id, now).ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "That drop is over. Drop the files again.",
        )
    })?;
    let done = import(to, &dropped.paths, &dropped.shown, trash, limits);
    if let Ok(Imported::Clashes(names)) = &done {
        dropped.shown.clone_from(names);
        drops.put_back(dropped);
    }
    done
}

/// Imports `sources`; `shown` are the clashing names the person was asked
/// about, the only ones `Clash::Replace` replaces.
pub(crate) fn import(
    to: &Target<'_>,
    sources: &[PathBuf],
    shown: &[String],
    trash: &Trash,
    limits: Limits,
) -> Result<Imported> {
    match plan(to, sources, shown, limits)? {
        Planned::Clashes(names) => Ok(Imported::Clashes(names)),
        Planned::Ready(plan) => run(to, &plan, trash, limits),
    }
}

struct Step<'a> {
    source: &'a Path,
    name: String,
    /// The name in the target folder (another one for Keep both).
    target: String,
    /// Trash the entry at `name` first.
    replace: bool,
}

pub(crate) struct Plan<'a> {
    /// The target folder, opened at the plan: the copy goes where the
    /// checks looked.
    dir: node::Dir,
    steps: Vec<Step<'a>>,
    links: Vec<String>,
}

pub(crate) enum Planned<'a> {
    Clashes(Vec<String>),
    Ready(Plan<'a>),
}

/// Looks at everything and decides every step, refusing what cannot be
/// done; changes nothing.
pub(crate) fn plan<'a>(
    to: &Target<'_>,
    sources: &'a [PathBuf],
    shown: &[String],
    limits: Limits,
) -> Result<Planned<'a>> {
    let (dir, handle) = target_dir(to.root, to.into)?;
    let mut scan = Walk::new(limits);
    // The source, its name, and the source with its parent resolved.
    let mut items: Vec<(&Path, String, PathBuf)> = Vec::new();
    let mut names = HashSet::new();
    for source in sources {
        let name = source_name(source, to.into)?;
        if !names.insert(fold(&name)) {
            return Err(bad(format!("Two dropped items are named {name}.")));
        }
        let meta = fs::symlink_metadata(source).map_err(|e| failure(&name, &e))?;
        if meta.file_type().is_symlink() {
            scan.links.push(name);
            continue;
        }
        let real = real_source(source, &name)?;
        edit::not_inside(&real, &dir)?;
        let (parent, in_parent) = node::parent_of(source).map_err(|e| failure(&name, &e))?;
        scan.visit(&parent, &in_parent, None, &name, 0)?;
        items.push((source, name, real));
    }
    let clashes: Vec<String> = items
        .iter()
        .filter(|(_, name, _)| present(&dir.join(name)))
        .map(|(_, name, _)| name.clone())
        .collect();
    if !clashes.is_empty() && to.clash == Clash::Ask {
        return Ok(Planned::Clashes(clashes));
    }
    let shown: HashSet<String> = shown.iter().map(|s| fold(s)).collect();
    let mut steps = Vec::new();
    for (source, name, real) in &items {
        let mut step = Step {
            source,
            name: name.clone(),
            target: name.clone(),
            replace: false,
        };
        if clashes.contains(name) {
            match to.clash {
                Clash::KeepBoth => {
                    step.target = copy_name(name, real.is_dir(), &dir, &names);
                    names.insert(fold(&step.target));
                }
                Clash::Replace if shown.contains(&fold(name)) => {
                    let old = dir.join(name);
                    if items.iter().any(|(_, _, real)| real.starts_with(&old)) {
                        return Err(bad(format!(
                            "{name} cannot replace itself or a folder a dropped item is in."
                        )));
                    }
                    step.replace = true;
                }
                Clash::Replace | Clash::Ask => return Err(exists(name)),
            }
        }
        steps.push(step);
    }
    Ok(Planned::Ready(Plan {
        dir: handle,
        steps,
        links: scan.links,
    }))
}

/// Carries out `plan`: trashes what it replaces, then copies, never over
/// an existing name.
pub(crate) fn run(
    to: &Target<'_>,
    plan: &Plan<'_>,
    trash: &Trash,
    limits: Limits,
) -> Result<Imported> {
    let mut copy = Walk::new(limits);
    let mut paths = Vec::new();
    for step in &plan.steps {
        if step.replace {
            edit::trash(to.root, &[to.into.join(&step.name)?], trash)?;
        }
        let fail = |e: io::Error| failure(&step.name, &e);
        let (parent, in_parent) = node::parent_of(step.source).map_err(fail)?;
        let target = (&plan.dir, OsStr::new(&step.target));
        copy.visit(&parent, &in_parent, Some(target), &step.name, 0)?;
        paths.push(to.into.join(&step.target)?);
    }
    // Also the links that replaced a file or folder since the plan.
    let mut skipped_links = plan.links.clone();
    for link in copy.links {
        if !skipped_links.contains(&link) {
            skipped_links.push(link);
        }
    }
    Ok(Imported::Copied {
        paths,
        skipped_links,
    })
}

fn bad(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

fn exists(name: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, format!("{name} already exists."))
}

/// An I/O error about the dropped entry `shown`, without the absolute path
/// an error of Ovra's own names.
fn failure(shown: &str, err: &io::Error) -> ProtocolError {
    match err.kind() {
        io::ErrorKind::AlreadyExists => exists(shown),
        _ if err.raw_os_error().is_some() => resolve::io_error(shown, err),
        _ => ProtocolError::new(
            ErrorCode::Internal,
            format!("{shown} changed while it was copied."),
        ),
    }
}

/// The key two names clash by on a file system that ignores case and
/// Unicode normalization (macOS, Windows).
fn fold(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// The existing folder `into`, inside the root, outside `.git` and not
/// through a link, so a folder an agent swapped for a link to another one
/// (`.ovra-attachments` for `src`, say) is refused. Also opened, so the
/// copy goes where these checks looked.
fn target_dir(root: &Path, into: &RelPath) -> Result<(PathBuf, node::Dir)> {
    let (root, real) = resolve::existing(root, into)?;
    if !real.is_dir() {
        return Err(bad(format!("{into} is not a folder.")));
    }
    edit::check_parent(&root, &real, into)?;
    let through_link = || {
        ProtocolError::new(
            ErrorCode::Forbidden,
            format!("{into} goes through a link; drop onto the folder it points to."),
        )
    };
    if real != into.under(&root) {
        return Err(through_link());
    }
    let handle = node::target(&root, into)
        .map_err(|e| resolve::io_error(into, &e))?
        .ok_or_else(through_link)?;
    Ok((real, handle))
}

/// The name a dropped `source` gets in `into`, refused unless it is a name
/// the project may hold. Messages name the item, not its full path.
fn source_name(source: &Path, into: &RelPath) -> Result<String> {
    let shown = source
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let unusable = || bad(format!("“{shown}” is not a name Ovra can use."));
    if !source.is_absolute() {
        return Err(bad(format!("{shown} is not a full path.")));
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(unusable)?;
    if name.eq_ignore_ascii_case(".git") {
        return Err(bad("A .git folder cannot be dropped into a project.".into()));
    }
    into.join(name).map_err(|_| unusable())?;
    Ok(name.to_owned())
}

fn real_source(source: &Path, name: &str) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or(source);
    let parent = fs::canonicalize(parent).map_err(|e| failure(name, &e))?;
    Ok(parent.join(name))
}

fn present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// A free name for a copy of `name` in `dir`, as the tree's Duplicate names
/// one (`copyName` in `ui/src/tree/ops.ts`), and not one of `taken`
/// (folded names).
fn copy_name(name: &str, is_dir: bool, dir: &Path, taken: &HashSet<String>) -> String {
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
        .find(|candidate| !taken.contains(&fold(candidate)) && !present(&dir.join(candidate)))
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

    /// Visits `name` in `dir`, and copies it to `to` (a folder and a name
    /// in it) when given one. Children are reached through their folder's
    /// handle, so a folder swapped for a link meanwhile is not followed.
    fn visit(
        &mut self,
        dir: &node::Dir,
        name: &OsStr,
        to: Option<(&node::Dir, &OsStr)>,
        shown: &str,
        depth: usize,
    ) -> Result<()> {
        let fail = |e: io::Error| failure(shown, &e);
        let meta = node::stat(dir, name).map_err(fail)?;
        if meta.kind == Kind::Link {
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
        match meta.kind {
            Kind::Dir => {
                if depth >= self.limits.depth {
                    return Err(bad(format!(
                        "{shown} is more than {} folders deep; Ovra copies at most that deep.",
                        self.limits.depth
                    )));
                }
                let from = node::open_dir(dir, name, &meta).map_err(fail)?;
                let made = match to {
                    Some((to, name)) => Some(node::make_dir(to, name).map_err(fail)?),
                    None => None,
                };
                #[cfg(test)]
                tests::opened(shown);
                for child in node::names(&from).map_err(fail)? {
                    let shown = format!("{shown}/{}", child.to_string_lossy());
                    let to = made.as_ref().map(|made| (made, child.as_os_str()));
                    self.visit(&from, &child, to, &shown, depth + 1)?;
                }
                // Last, so a read-only folder still gets its children.
                if let Some(made) = &made {
                    node::set_perms(made, &meta.perms).map_err(fail)?;
                }
                Ok(())
            }
            Kind::File => {
                let Some((to, to_name)) = to else {
                    return self.add_bytes(meta.len);
                };
                let source = node::open_file(dir, name, &meta).map_err(fail)?;
                let mut target = node::make_file(to, to_name).map_err(fail)?;
                let copied = self.copy_file(source, &mut target, &meta.perms, shown);
                if copied.is_err() {
                    // Ours (made new), and only part of the source.
                    drop(target);
                    let _ = node::remove_file(to, to_name);
                }
                copied
            }
            Kind::Link | Kind::Other => Err(bad(format!(
                "{shown} is not a file or folder (a device, pipe or socket), so Ovra does not copy it."
            ))),
        }
    }

    fn copy_file(
        &mut self,
        source: fs::File,
        target: &mut fs::File,
        perms: &fs::Permissions,
        shown: &str,
    ) -> Result<()> {
        // A file that grew since the scan stops at the limit.
        let left = self.limits.bytes.saturating_sub(self.bytes);
        let copied =
            io::copy(&mut source.take(left + 1), target).map_err(|e| failure(shown, &e))?;
        self.add_bytes(copied)?;
        target
            .set_permissions(perms.clone())
            .map_err(|e| failure(shown, &e))
    }
}

#[cfg(test)]
mod tests;
