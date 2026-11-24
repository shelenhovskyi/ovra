//! `restore`, `apply` and `commit`: changes to exactly the files named.
//!
//! These run on the person's main checkout too, so they are careful: a path
//! that is a directory (on disk or in `base`), or that lies under a
//! symlinked directory, is skipped and left for the next `changes` to show.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
/// Paths that are trees in `base` are left out. The whole tree is listed and
/// filtered here: hundreds of paths do not fit a Windows command line.
fn blobs(dir: &Path, base: &str, paths: &[RelPath]) -> Result<BTreeMap<String, String>> {
    let out = Git::new(dir, "ls-tree").args(["-r", "-z", base]).run()?;
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
    copy_over(from, into, paths, Some(&blobs(into, &base, paths)?))
}

/// Copies `paths` from `from` into `into` whatever `into` holds; returns
/// the paths it could not write safely.
pub(crate) fn take(from: &Path, into: &Path, paths: &[RelPath]) -> Result<Vec<String>> {
    copy_over(from, into, paths, None)
}

/// Copies `paths` from `from` into `into`. With `expected` (blob ids by
/// path; a missing entry means no file), a file of `into` that differs is
/// skipped and returned as a conflict.
fn copy_over(
    from: &Path,
    into: &Path,
    paths: &[RelPath],
    expected: Option<&BTreeMap<String, String>>,
) -> Result<Vec<String>> {
    let mut present = Vec::new();
    let mut conflicts = Vec::new();
    let mut todo = Vec::new();
    for path in paths {
        let (here, there) = (on_disk(into, path)?, on_disk(from, path)?);
        let source = match there {
            OnDisk::Off => None,
            OnDisk::Missing => Some(Source::Missing),
            OnDisk::File => source(from, path),
        };
        match (here, source) {
            (OnDisk::Off, _) | (_, None) => conflicts.push(path.to_string()),
            // A symlink in the checkout is left for a person: its hash would
            // be of what it points to, and copying would write through it.
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
        let to = path.under(into);
        match source {
            Source::File => place(&to, |temp| copy_file(&path.under(from), temp))?,
            Source::Link(target) => place(&to, |temp| make_link(&target, temp))?,
            Source::Missing => remove_file(into, path)?,
        }
    }
    conflicts.sort();
    Ok(conflicts)
}

/// What [`apply`] puts at a path.
enum Source {
    File,
    /// A symlink with this target, read once and checked by [`link_stays`].
    Link(PathBuf),
    Missing,
}

/// What the worker has at `path` (a file or a symlink); `None` for a symlink
/// that cannot be recreated safely: on Windows any, since copying it would
/// copy what it points to, and on Unix one that may leave the checkout.
fn source(from: &Path, path: &RelPath) -> Option<Source> {
    if !is_symlink(from, path) {
        return Some(Source::File);
    }
    let target = fs::read_link(path.under(from)).ok()?;
    (cfg!(unix) && link_stays(path, &target)).then_some(Source::Link(target))
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

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Puts what `write` creates at `to`, creating parent directories. `write`
/// creates a new temporary file next to `to` (and removes it if it fails),
/// which is then renamed over `to`: a rename replaces a symlink at `to`
/// instead of writing through it. A temporary name already taken is never
/// touched; the next one is tried.
fn place(to: &Path, write: impl Fn(&Path) -> io::Result<()>) -> Result<()> {
    let fail = |e: io::Error| internal(format!("cannot copy to {}: {e}", to.display()));
    let parent = to.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(fail)?;
    for _ in 0..16 {
        let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".ovra-copy-{}-{n}", std::process::id()));
        match write(&temp) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(fail(e)),
            Ok(()) => {}
        }
        let renamed = fs::rename(&temp, to);
        if renamed.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return renamed.map_err(fail);
    }
    Err(fail(ErrorKind::AlreadyExists.into()))
}

/// Copies the regular file `from` to the new file `temp`. A symlink at
/// `from`, even one swapped in after [`apply`] looked, is refused: the file
/// opened must be the one `from` names without following links.
fn copy_file(from: &Path, temp: &Path) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let meta = source.metadata()?;
    if !meta.is_file() || !same_file(&meta, &fs::symlink_metadata(from)?) {
        return Err(io::Error::other("the source changed while it was copied"));
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut copy = options.open(temp)?;
    let copied =
        io::copy(&mut source, &mut copy).and_then(|_| copy.set_permissions(meta.permissions()));
    if copied.is_err() {
        drop(copy);
        let _ = fs::remove_file(temp);
    }
    copied
}

/// Whether `opened` (of a handle) and `named` (of the path, not followed)
/// are the same file.
#[cfg(unix)]
fn same_file(opened: &fs::Metadata, named: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    !named.is_symlink() && opened.dev() == named.dev() && opened.ino() == named.ino()
}

#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, named: &fs::Metadata) -> bool {
    !named.is_symlink()
}

#[cfg(unix)]
fn make_link(target: &Path, temp: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, temp)
}

#[cfg(not(unix))]
fn make_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other("symlinks are recreated on Unix only"))
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
    fn place_replaces_a_symlink_at_the_target_and_skips_taken_names() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = tmp.path();
        let (from, to, outside) = (dir.join("from"), dir.join("to"), dir.join("outside"));
        fs::write(&from, "new\n").unwrap_or_else(|e| panic!("{e}"));
        fs::write(&outside, "outside\n").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(&outside, &to).unwrap_or_else(|e| panic!("{e}"));
        // The next temporary name is taken: it is left alone.
        let next = NEXT_TEMP.load(Ordering::Relaxed);
        let taken = dir.join(format!(".ovra-copy-{}-{next}", std::process::id()));
        fs::write(&taken, "planted\n").unwrap_or_else(|e| panic!("{e}"));

        place(&to, |temp| copy_file(&from, temp)).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            fs::read_to_string(&outside).ok().as_deref(),
            Some("outside\n")
        );
        assert_eq!(fs::read_to_string(&to).ok().as_deref(), Some("new\n"));
        assert_eq!(
            fs::read_to_string(&taken).ok().as_deref(),
            Some("planted\n")
        );
        let count = fs::read_dir(dir).map(Iterator::count).unwrap_or_default();
        assert_eq!(count, 4, "no temporary file of ours is left");
    }

    #[test]
    fn copy_file_refuses_a_symlink_source() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (link, temp) = (tmp.path().join("link"), tmp.path().join("temp"));
        fs::write(tmp.path().join("real"), "x").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink("real", &link).unwrap_or_else(|e| panic!("{e}"));
        assert!(copy_file(&link, &temp).is_err());
        assert!(!temp.exists());
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
