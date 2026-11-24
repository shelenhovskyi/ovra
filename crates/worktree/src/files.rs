//! `restore`, `apply` and `commit`: changes to exactly the files named.
//!
//! These run on the person's main checkout too, so they are careful: a path
//! that is a directory (on disk or in `base`), or that lies under a
//! symlinked directory, is skipped and left for the next `changes` to show.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

use crate::git::{self, Git, TempIndex, internal};

/// What is at a path on disk, without following a symlink at the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDisk {
    Missing,
    /// A regular file or a symlink.
    File,
    /// A directory, or a path under a symlinked directory: never touched.
    Off,
}

fn on_disk(dir: &Path, path: &RelPath) -> Result<OnDisk> {
    let mut at = dir.to_path_buf();
    let parts: Vec<&str> = path.as_str().split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        at.push(part);
        let meta = match fs::symlink_metadata(&at) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(OnDisk::Missing),
            Err(e) => return Err(internal(format!("{}: {e}", at.display()))),
        };
        // A symlink is not `is_dir`: a symlinked parent is off too.
        if i + 1 < parts.len() && !meta.is_dir() {
            return Ok(OnDisk::Off);
        }
        if i + 1 == parts.len() && meta.is_dir() {
            return Ok(OnDisk::Off);
        }
    }
    Ok(OnDisk::File)
}

/// `(mode, id)` of the entries among `paths` in the commit or tree `tree`,
/// keyed by path; folders are left out, submodules are kept (mode 160000).
/// The whole tree is listed and filtered here: hundreds of paths do not fit
/// a Windows command line.
fn entries(
    dir: &Path,
    tree: &str,
    paths: &[RelPath],
) -> Result<BTreeMap<String, (String, String)>> {
    let out = Git::new(dir, "ls-tree")
        .args(["-r", "-z", "--end-of-options", tree])
        .run()?;
    let wanted: BTreeSet<&str> = paths.iter().map(RelPath::as_str).collect();
    let mut found = BTreeMap::new();
    for record in git::fields(&out) {
        // `<mode> <type> <id>\t<path>`
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let mut meta = meta.split(' ');
        let (Some(mode), Some(_kind), Some(id)) = (meta.next(), meta.next(), meta.next()) else {
            continue;
        };
        if wanted.contains(path) {
            found.insert(path.to_owned(), (mode.to_owned(), id.to_owned()));
        }
    }
    Ok(found)
}

/// Blob ids of the files and symlinks among `paths` in commit `base`.
fn blobs(dir: &Path, base: &str, paths: &[RelPath]) -> Result<BTreeMap<String, String>> {
    Ok(entries(dir, base, paths)?
        .into_iter()
        .filter(|(_, (mode, _))| mode != "160000")
        .map(|(path, (_, id))| (path, id))
        .collect())
}

fn remove_file(dir: &Path, path: &RelPath) -> Result<()> {
    match fs::remove_file(path.under(dir)) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            Err(internal(format!("cannot delete {path}: {e}")))
        }
        _ => Ok(()),
    }
}

pub(crate) fn restore(dir: &Path, base: &str, paths: &[RelPath]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let base = git::commit_id(dir, base)?;
    let in_base = blobs(dir, &base, paths)?;
    let mut checkout = Vec::new();
    for path in paths {
        let disk = on_disk(dir, path)?;
        if disk == OnDisk::Off {
            continue;
        }
        if in_base.contains_key(path.as_str()) {
            checkout.push(path.as_str().as_bytes());
        } else if disk == OnDisk::File {
            remove_file(dir, path)?;
        }
    }
    if checkout.is_empty() {
        return Ok(());
    }
    let index = TempIndex::empty(dir)?;
    Git::new(dir, "read-tree").arg(&base).index(&index).run()?;
    Git::new(dir, "checkout-index")
        .args(["--force", "-z", "--stdin"])
        .index(&index)
        .input(checkout.join(&0))
        .run()?;
    Ok(())
}

/// Writes `paths` as they are in `tree` (a snapshot or look of a worker's
/// worktree in the same repository) into `into`, unless the file in `into`
/// no longer matches `base`: those are returned as conflicts.
pub(crate) fn apply(tree: &str, base: &str, into: &Path, paths: &[RelPath]) -> Result<Vec<String>> {
    let base = git::commit_id(into, base)?;
    copy_over(tree, into, paths, Some(&blobs(into, &base, paths)?))
}

/// Writes `paths` as they are in `tree` into `into` whatever `into` holds;
/// returns the paths it could not write safely.
pub(crate) fn take(tree: &str, into: &Path, paths: &[RelPath]) -> Result<Vec<String>> {
    copy_over(tree, into, paths, None)
}

/// Writes `paths` from `tree` into `into`: the versions someone reviewed,
/// never what is on a worker's disk by then. With `expected` (blob ids by
/// path; a missing entry means no file), a file of `into` that differs is
/// skipped and returned as a conflict.
fn copy_over(
    tree: &str,
    into: &Path,
    paths: &[RelPath],
    expected: Option<&BTreeMap<String, String>>,
) -> Result<Vec<String>> {
    // Paths are relative to the top of a worktree; in a checkout below its
    // repository's top they would name other files.
    if !git::dirs(into)?.prefix.is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::Conflict,
            "copying a worker's files needs the project at the top of its repository",
        ));
    }
    let wanted = entries(into, tree, paths)?;
    let mut present = Vec::new();
    let mut conflicts = Vec::new();
    let mut todo = Vec::new();
    for path in paths {
        let here = on_disk(into, path)?;
        let source = match wanted.get(path.as_str()) {
            None => Some(Source::Missing),
            Some((mode, _)) if mode == "100644" || mode == "100755" => Some(Source::Written),
            Some((mode, id)) if mode == "120000" => link(into, path, id)?,
            // A submodule: not a file to copy.
            Some(_) => None,
        };
        match (here, source) {
            (OnDisk::Off, _) | (_, None) => conflicts.push(path.to_string()),
            // A symlink in the checkout is left for a person: its hash would
            // be of what it points to.
            (OnDisk::File, _) if is_symlink(into, path) => conflicts.push(path.to_string()),
            (here, Some(source)) => {
                if here == OnDisk::File {
                    present.push(path);
                }
                todo.push((path, here, source));
            }
        }
    }
    // Hashed the way `git add` would, so line-ending filters match.
    let hashes = if present.is_empty() || expected.is_none() {
        Vec::new()
    } else {
        let list: Vec<&str> = present.iter().map(|p| p.as_str()).collect();
        let out = Git::new(into, "hash-object")
            .arg("--stdin-paths")
            .input(list.join("\n") + "\n")
            .text()?;
        out.lines().map(str::to_owned).collect()
    };
    let current: BTreeMap<&str, &String> = present
        .iter()
        .map(|p| p.as_str())
        .zip(hashes.iter())
        .collect();
    // `<mode> <id>\t<path>` of what is written, and what is deleted.
    let mut checkout = Vec::new();
    let mut info = Vec::new();
    let mut deleted = Vec::new();
    for (path, here, source) in todo {
        let key = path.as_str();
        // Looked at again just before writing: the path or a folder on the
        // way may have changed since (an agent in direct mode works here
        // too). What changes between this look and the write is not seen.
        let changed = expected.is_some_and(|e| current.get(key).copied() != e.get(key));
        if changed || on_disk(into, path)? != here || is_symlink(into, path) {
            conflicts.push(key.to_owned());
            continue;
        }
        match (source, wanted.get(key)) {
            (Source::Written, Some((mode, id))) => {
                checkout.push(key.as_bytes());
                info.extend_from_slice(format!("{mode} {id}\t{key}").as_bytes());
                info.push(0);
            }
            (Source::Written, None) => conflicts.push(key.to_owned()),
            (Source::Missing, _) => deleted.push(path),
        }
    }
    if !checkout.is_empty() {
        // An index of just the files written: git then reads attributes
        // (line endings, filters) only from the checkout and from
        // `.gitattributes` files that are themselves written, never from
        // the rest of the worker's tree, which nobody reviewed.
        // `checkout-index` unlinks a file or symlink at the path instead of
        // writing through it, and applies the checkout's filters.
        let index = TempIndex::empty(into)?;
        Git::new(into, "update-index")
            .args(["-z", "--index-info"])
            .index(&index)
            .input(info)
            .run()?;
        Git::new(into, "checkout-index")
            .args(["--force", "-z", "--stdin"])
            .index(&index)
            .input(checkout.join(&0))
            .run()?;
    }
    // Deleted only once the writes went through, so a failure leaves the
    // checkout as it was as far as deletions go.
    for path in deleted {
        remove_file(into, path)?;
    }
    conflicts.sort();
    Ok(conflicts)
}

/// What [`copy_over`] puts at a path.
enum Source {
    /// A file or a symlink, written from the tree.
    Written,
    Missing,
}

/// A symlink of the tree, if it can be recreated safely: on Windows none,
/// since git would write it as a file, and on Unix one that stays inside.
fn link(dir: &Path, path: &RelPath, id: &str) -> Result<Option<Source>> {
    if !cfg!(unix) {
        return Ok(None);
    }
    let target = Git::new(dir, "cat-file").args(["blob", id]).run()?;
    let target = String::from_utf8_lossy(&target);
    Ok(link_stays(path, Path::new(target.as_ref())).then_some(Source::Written))
}

/// Whether `path` under `dir` is a symlink itself (not followed).
fn is_symlink(dir: &Path, path: &RelPath) -> bool {
    fs::symlink_metadata(path.under(dir)).is_ok_and(|m| m.is_symlink())
}

/// Whether a symlink at `path` with `target` stays inside the checkout,
/// judged lexically: relative, and its `..` only at the start, where they
/// climb the real folders above the link (`apply` refuses symlinked
/// folders). A `..` after a name could follow another link first
/// (`s -> .`, then `l -> s/../x`), so it is refused.
fn link_stays(path: &RelPath, target: &Path) -> bool {
    let mut up = path.as_str().matches('/').count();
    let mut named = false;
    for component in target.components() {
        match component {
            Component::Normal(_) => named = true,
            Component::CurDir => {}
            Component::ParentDir if !named => match up.checked_sub(1) {
                Some(n) => up = n,
                None => return false,
            },
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

pub(crate) fn commit(dir: &Path, paths: &[RelPath], message: &str) -> Result<String> {
    if paths.is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "no files to commit",
        ));
    }
    let head = git::head(dir)?;
    // Start from HEAD, not from the real index: whatever else the person
    // staged stays staged and out of this commit.
    let index = TempIndex::empty(dir)?;
    if let Some(head) = &head {
        Git::new(dir, "read-tree").arg(head).index(&index).run()?;
    }
    let list: Vec<&[u8]> = paths.iter().map(|p| p.as_str().as_bytes()).collect();
    Git::new(dir, "update-index")
        .args(["--add", "--remove", "-z", "--stdin"])
        .index(&index)
        .input(list.join(&0))
        .run()?;
    let tree = Git::new(dir, "write-tree").index(&index).text()?;
    let unchanged = match &head {
        Some(head) => {
            Git::new(dir, "rev-parse")
                .arg(format!("{head}^{{tree}}"))
                .text()?
                == tree
        }
        None => false,
    };
    if unchanged {
        return Err(ProtocolError::new(
            ErrorCode::Conflict,
            "the files have no changes to commit",
        ));
    }
    let mut new = Git::new(dir, "commit-tree").arg(&tree);
    if let Some(head) = &head {
        new = new.args(["-p", head]);
    }
    let id = new.args(["-F", "-"]).input(message).text()?;
    let subject = message.lines().next().unwrap_or_default();
    Git::new(dir, "update-ref")
        .args(["-m", &format!("commit: {subject}"), "HEAD", &id])
        .arg(head.as_deref().unwrap_or(""))
        .run()?;
    // The committed files are now staged as committed, like `git commit
    // <paths>` leaves them.
    Git::new(dir, "reset")
        .args([
            "--quiet",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
            &id,
        ])
        .input(list.join(&0))
        .run()?;
    Ok(id)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn rel(raw: &str) -> RelPath {
        RelPath::parse(raw).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_link_stays_inside_only_when_its_dots_lead() {
        for (at, target, stays) in [
            ("a/b/l", "../x", true),
            ("a/b/l", "../../x", true),
            ("a/b/l", "../../../x", false),
            ("l", "x/./y", true),
            ("l", "s/../x", false),
            ("l", "s/s/s/../../../x", false),
            ("l", "/etc/passwd", false),
        ] {
            assert_eq!(
                link_stays(&rel(at), Path::new(target)),
                stays,
                "{at} -> {target}"
            );
        }
    }
}
