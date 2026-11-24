//! `restore`, `apply` and `commit`: changes to exactly the files named.
//!
//! These run on the person's main checkout too, so they are careful: a path
//! that is a directory (on disk or in `base`), or that lies under a
//! symlinked directory, is skipped and left for the next `changes` to show.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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

/// Blob ids of the files among `paths` in commit `base`, keyed by path.
/// Paths that are trees in `base` are left out.
fn blobs(dir: &Path, base: &str, paths: &[RelPath]) -> Result<BTreeMap<String, String>> {
    let out = Git::new(dir, "ls-tree")
        .args(["-r", "-z", base])
        .paths(paths)
        .run()?;
    let wanted: BTreeSet<&str> = paths.iter().map(RelPath::as_str).collect();
    let mut found = BTreeMap::new();
    for record in git::fields(&out) {
        // `<mode> <type> <id>\t<path>`
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let mut meta = meta.split(' ');
        let (Some(_mode), Some("blob"), Some(id)) = (meta.next(), meta.next(), meta.next()) else {
            continue;
        };
        if wanted.contains(path) {
            found.insert(path.to_owned(), id.to_owned());
        }
    }
    Ok(found)
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

pub(crate) fn apply(
    from: &Path,
    base: &str,
    into: &Path,
    paths: &[RelPath],
) -> Result<Vec<String>> {
    let base = git::commit_id(into, base)?;
    let expected = blobs(into, &base, paths)?;
    let mut present = Vec::new();
    let mut conflicts = Vec::new();
    let mut todo = Vec::new();
    for path in paths {
        match (on_disk(into, path)?, on_disk(from, path)?) {
            (OnDisk::Off, _) | (_, OnDisk::Off) => conflicts.push(path.to_string()),
            (OnDisk::File, source) => {
                present.push(path);
                todo.push((path, source));
            }
            (OnDisk::Missing, source) => todo.push((path, source)),
        }
    }
    // Hashed the way `git add` would, so line-ending filters match.
    let hashes = if present.is_empty() {
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
    for (path, source) in todo {
        let key = path.as_str();
        if current.get(key).copied() != expected.get(key) {
            conflicts.push(key.to_owned());
            continue;
        }
        match source {
            OnDisk::File => copy(&path.under(from), &path.under(into))?,
            OnDisk::Missing => remove_file(into, path)?,
            OnDisk::Off => {}
        }
    }
    conflicts.sort();
    Ok(conflicts)
}

/// Copies one file, or recreates a symlink, creating parent directories.
fn copy(from: &Path, to: &Path) -> Result<()> {
    let fail = |e: std::io::Error| internal(format!("cannot copy to {}: {e}", to.display()));
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    #[cfg(unix)]
    if fs::symlink_metadata(from).map_err(fail)?.is_symlink() {
        let target: PathBuf = fs::read_link(from).map_err(fail)?;
        match fs::remove_file(to) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(fail(e)),
            _ => {}
        }
        return std::os::unix::fs::symlink(target, to).map_err(fail);
    }
    fs::copy(from, to).map(|_| ()).map_err(fail)
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
        .args(["--quiet", &id])
        .paths(paths)
        .run()?;
    Ok(id)
}
