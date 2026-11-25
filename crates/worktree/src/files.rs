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
pub(crate) enum OnDisk {
    Missing,
    /// A regular file or a symlink.
    File,
    /// A directory, or a path under a symlinked directory: never touched.
    Off,
}

pub(crate) fn on_disk(dir: &Path, path: &RelPath) -> Result<OnDisk> {
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

/// Deletes the file or link at `path`; `false` if it was left because a
/// folder on the way is not a real folder any more. On Unix each folder is
/// opened without following links, so a folder an agent swaps for a link
/// after [`on_disk`] looked never makes Ovra delete outside `dir`.
#[cfg(unix)]
pub(crate) fn remove_file(dir: &Path, path: &RelPath) -> Result<bool> {
    use rustix::fs::{AtFlags, Mode, OFlags, open, openat, unlinkat};
    use rustix::io::Errno;
    let folder = OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY;
    let mut parts: Vec<&str> = path.as_str().split('/').collect();
    let Some(name) = parts.pop() else {
        return Ok(true);
    };
    let removed = open(dir, OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty()).and_then(|top| {
        let mut at = top;
        for part in parts {
            at = openat(&at, part, folder, Mode::empty())?;
        }
        unlinkat(&at, name, AtFlags::empty())
    });
    match removed {
        Ok(()) | Err(Errno::NOENT) => Ok(true),
        Err(Errno::LOOP | Errno::NOTDIR | Errno::ISDIR) => Ok(false),
        Err(e) => Err(internal(format!("cannot delete {path}: {e}"))),
    }
}

#[cfg(not(unix))]
pub(crate) fn remove_file(dir: &Path, path: &RelPath) -> Result<bool> {
    match fs::remove_file(path.under(dir)) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            Err(internal(format!("cannot delete {path}: {e}")))
        }
        _ => Ok(true),
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
            // Left for the next `changes` to show, as `Off` paths are.
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
        if !plain(path) {
            conflicts.push(path.to_string());
            continue;
        }
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
        let hashes: Vec<String> = out.lines().map(str::to_owned).collect();
        if hashes.len() != present.len() {
            return Err(internal("git hashed a different number of files"));
        }
        hashes
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
        let count = checkout.len();
        Git::new(into, "update-index")
            .args(["-z", "--index-info"])
            .index(&index)
            .input(info)
            .run()?;
        // `update-index` skips entries git refuses; `checkout-index` would
        // then write the rest and fail. Nothing is written unless all are in.
        let staged = Git::new(into, "ls-files").arg("-z").index(&index).run()?;
        if staged.split(|b| *b == 0).filter(|p| !p.is_empty()).count() != count {
            return Err(internal("git refused some of the files to write"));
        }
        Git::new(into, "checkout-index")
            .args(["--force", "-z", "--stdin"])
            .index(&index)
            .input(checkout.join(&0))
            .run()?;
    }
    // Deleted only once the writes went through, so a failure leaves the
    // checkout as it was as far as deletions go. Each path is looked at
    // again right before: a folder on the way may have become a symlink.
    // A deletion that names a written file on a case-insensitive disk
    // would remove it again, so it is left to the person.
    let written: BTreeSet<String> = checkout
        .iter()
        .map(|p| String::from_utf8_lossy(p).to_lowercase())
        .collect();
    for path in deleted {
        let folded = written.contains(&path.as_str().to_lowercase());
        match on_disk(into, path)? {
            OnDisk::Missing => {}
            OnDisk::File if !folded && !is_symlink(into, path) => {
                if !remove_file(into, path)? {
                    conflicts.push(path.to_string());
                }
            }
            OnDisk::File | OnDisk::Off => conflicts.push(path.to_string()),
        }
    }
    conflicts.sort();
    Ok(conflicts)
}

/// Whether `path` is safe to hand to git and to delete: no `.git` folder in
/// any of the spellings git refuses on some disk (case, a trailing dot or
/// space, the 8.3 name), no line breaks, no leading quote (`hash-object
/// --stdin-paths` would unquote it) and no replacement character (a name
/// that was not UTF-8 in the tree).
fn plain(path: &RelPath) -> bool {
    let text = path.as_str();
    if text.contains(['\n', '\r', '\u{fffd}']) || text.starts_with('"') {
        return false;
    }
    !text.split('/').any(|part| {
        let part = part.trim_end_matches(['.', ' ']).to_ascii_lowercase();
        part == ".git" || part == "git~1"
    })
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
    Ok(link_stays(path, &target).then_some(Source::Written))
}

/// Whether `path` under `dir` is a symlink itself (not followed).
fn is_symlink(dir: &Path, path: &RelPath) -> bool {
    fs::symlink_metadata(path.under(dir)).is_ok_and(|m| m.is_symlink())
}

/// Whether a symlink at `path` with `target` stays inside the checkout,
/// judged lexically: relative, and its `..` only at the start, where they
/// climb the real folders above the link (`apply` refuses symlinked
/// folders). A `..` after a name could follow another link first
/// (`s -> .`, then `l -> s/../x`), so it is refused. `target` is the blob's
/// raw bytes: git writes a link only up to a NUL, so a target with one, an
/// empty one or one that is not UTF-8 is refused rather than judged.
fn link_stays(path: &RelPath, target: &[u8]) -> bool {
    let Ok(target) = std::str::from_utf8(target) else {
        return false;
    };
    if target.is_empty() || target.contains('\0') {
        return false;
    }
    let target = Path::new(target);
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
    fn a_delete_never_follows_a_folder_swapped_for_a_link() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (dir, outside) = (root.path().join("work"), root.path().join("outside"));
        for folder in [&dir.join("real"), &outside] {
            fs::create_dir_all(folder).unwrap_or_else(|e| panic!("{e}"));
        }
        fs::write(outside.join("keep.txt"), "mine").unwrap_or_else(|e| panic!("{e}"));
        fs::write(dir.join("real/gone.txt"), "x").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(&outside, dir.join("swapped")).unwrap_or_else(|e| panic!("{e}"));
        let removed = remove_file(&dir, &rel("swapped/keep.txt"));
        assert!(!removed.unwrap_or_else(|e| panic!("{}", e.message)));
        assert!(outside.join("keep.txt").exists());
        let removed = remove_file(&dir, &rel("real/gone.txt"));
        assert!(removed.unwrap_or_else(|e| panic!("{}", e.message)));
        assert!(!dir.join("real/gone.txt").exists());
        // The link itself is a file to delete.
        remove_file(&dir, &rel("swapped")).unwrap_or_else(|e| panic!("{}", e.message));
        assert!(!dir.join("swapped").exists() && outside.join("keep.txt").exists());
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
            // Git would write these only up to the NUL: `..`, and `x`
            // after climbing out.
            ("l", "..\0x", false),
            ("a/l", "x\0/../../..", false),
            ("l", "", false),
        ] {
            assert_eq!(
                link_stays(&rel(at), target.as_bytes()),
                stays,
                "{at} -> {target}"
            );
        }
    }
}
