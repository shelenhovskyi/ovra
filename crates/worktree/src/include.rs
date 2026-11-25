//! Copying the ignored files `.worktreeinclude` lists from the main checkout
//! into a new worktree (`docs/specs/worktree-setup.md`).
//!
//! Every folder on the way is opened without following links, on both
//! sides: an agent in the main checkout may swap a folder for a link at any
//! moment, and Ovra must neither read nor write through one.

#[cfg(unix)]
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read as _, Seek as _};
use std::path::Path;

use ovra_core::services::{Included, Result, SkippedInclude};
use sha2::{Digest as _, Sha256};

#[cfg(unix)]
use crate::git::{self, Git};

const LIST: &str = ".worktreeinclude";

/// The most bytes plain (not cloned) copies may take in all.
const PLAIN_CAP: u64 = 256 << 20;

#[cfg(unix)]
const SYMLINK: &str = "a symlink";
#[cfg(unix)]
const NO_CLONE: &str = "a folder, and this volume cannot clone";

pub(crate) fn include(root: &Path, dir: &Path) -> Result<Included> {
    copy_in(root, dir, PLAIN_CAP, clone)
}

/// Names `file`'s content: the hex SHA-256 of what it holds, read from the
/// start.
pub(crate) fn hash(file: &mut File) -> io::Result<String> {
    file.rewind()?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        match file.read(&mut chunk)? {
            0 => break,
            n => hasher.update(&chunk[..n]),
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// A copy-on-write clone of `from` into the empty `to`.
type Clone = fn(&File, &File) -> io::Result<()>;

#[cfg(target_os = "linux")]
fn clone(from: &File, to: &File) -> io::Result<()> {
    rustix::fs::ioctl_ficlone(to, from).map_err(io::Error::from)
}

#[cfg(not(target_os = "linux"))]
fn clone(_: &File, _: &File) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// The paths to copy: matched by `list` (the lines of `.worktreeinclude`)
/// and ignored by git. Folders a folder line matched end in `/`.
#[cfg(unix)]
fn listed(root: &Path, list: &str) -> Result<Vec<String>> {
    // Patterns as git reads them from a file: no blank lines or comments,
    // trailing spaces dropped unless escaped.
    let lines: Vec<&str> = list
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .map(|l| match l.trim_end_matches(' ') {
            t if t.ends_with('\\') && t.len() < l.len() => &l[..=t.len()],
            t => t,
        })
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    // Untracked paths `lines` match; with `-x` alone git's own rules do not
    // count. `--directory` names a folder whose every path matches once
    // instead of file by file.
    let matching = |lines: &[&str], whole: bool, under: Option<&str>| -> Result<Vec<String>> {
        let mut git = Git::new(root, "ls-files").args(["-z", "--others", "--ignored"]);
        for line in lines {
            git = git.arg(format!("--exclude={line}"));
        }
        if whole {
            git = git.args(["--directory", "--no-empty-directory"]);
        }
        if let Some(under) = under {
            git = git.args(["--", under]);
        }
        Ok(git::fields(&git.run()?).collect())
    };
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let list = &lines[..];
    let folder_lines: Vec<&str> = lines.iter().copied().filter(|l| l.ends_with('/')).collect();
    let folders = match folder_lines.is_empty() {
        true => Vec::new(),
        false => matching(&folder_lines, true, None)?,
    };
    let in_folder = |p: &str| folders.iter().any(|f| p.starts_with(f.as_str()));
    let mut paths = folders.clone();
    for path in matching(list, true, None)? {
        if path.ends_with('/') && !in_folder(&path) {
            // Every path in it matched a file line (`sub/.env` alone in `sub`).
            let files = matching(list, false, Some(&path))?;
            paths.extend(files.into_iter().filter(|f| !in_folder(f)));
        } else if !in_folder(&path) {
            paths.push(path);
        }
    }
    let out = Git::new(root, "ls-files")
        .args(["-z", "--others", "--ignored", "--exclude-standard"])
        .args(["--directory", "--no-empty-directory"])
        .run()?;
    let ignored: BTreeSet<String> = git::fields(&out).collect();
    let is_ignored = |path: &str| {
        ignored.contains(path)
            || path
                .match_indices('/')
                .any(|(end, _)| ignored.contains(&path[..=end]))
    };
    let mut picked = BTreeSet::new();
    for path in paths {
        if is_ignored(&path) {
            picked.insert(path);
        } else if path.ends_with('/') {
            // A folder git ignores only part of (`config/` with an ignored
            // `.env` beside tracked files): those parts.
            picked.extend(ignored.iter().filter(|i| i.starts_with(&path)).cloned());
        }
    }
    // Sorted, a folder comes right before what is in it.
    let mut kept: Vec<String> = Vec::new();
    for path in picked {
        if !kept
            .last()
            .is_some_and(|k| k.ends_with('/') && path.starts_with(k))
        {
            kept.push(path);
        }
    }
    Ok(kept)
}

#[cfg(not(unix))]
fn copy_in(root: &Path, _: &Path, _: u64, _: Clone) -> Result<Included> {
    let mut included = Included::default();
    if std::fs::symlink_metadata(root.join(LIST)).is_ok() {
        let reason = "copying it is not available on this system yet";
        included.skipped.push(skip(LIST, reason));
    }
    Ok(included)
}

#[cfg(unix)]
fn copy_in(root: &Path, dir: &Path, cap: u64, clone: Clone) -> Result<Included> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use rustix::io::Errno;
    let mut included = Included::default();
    let top = |path: &Path| {
        open(path, OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| git::internal(format!("cannot open {}: {e}", path.display())))
    };
    let from = top(root)?;
    let list = match openat(&from, LIST, unix::SOURCE, Mode::empty()) {
        Ok(fd) => io::read_to_string(File::from(fd)),
        Err(Errno::NOENT) => return Ok(included),
        Err(Errno::LOOP) => {
            included.skipped.push(skip(LIST, SYMLINK));
            return Ok(included);
        }
        Err(e) => Err(e.into()),
    };
    let list = list.map_err(|e| git::internal(format!("cannot read {LIST}: {e}")))?;
    let paths = listed(root, &list)?;
    let mut copy = unix::Copy {
        from,
        to: top(dir)?,
        dir,
        cap,
        plain: 0,
        clone,
        can_clone: None,
        out: &mut included,
    };
    for path in &paths {
        if path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"))
        {
            copy.out.skipped.push(skip(path, "under .git"));
        } else if let Some(folder) = path.strip_suffix('/') {
            copy.folder(folder);
        } else {
            copy.file(path);
        }
    }
    Ok(included)
}

fn skip(path: &str, reason: &str) -> SkippedInclude {
    SkippedInclude {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io;
    use std::os::fd::OwnedFd;
    use std::path::Path;

    use ovra_core::services::{Included, IncludedFile};
    use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, mkdirat, openat, unlinkat};
    use rustix::io::Errno;

    use super::{Clone, NO_CLONE, SYMLINK, hash, skip};

    const FOLDER: OFlags = OFlags::DIRECTORY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC)
        .union(OFlags::RDONLY);
    /// Never blocks on a FIFO; only regular files are read.
    pub(super) const SOURCE: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC)
        .union(OFlags::NONBLOCK);
    const TARGET: OFlags = OFlags::RDWR
        .union(OFlags::CREATE)
        .union(OFlags::EXCL)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    /// One run of [`super::copy_in`].
    pub(super) struct Copy<'a> {
        pub(super) from: OwnedFd,
        pub(super) to: OwnedFd,
        /// The worktree `to` is open at, made by Ovra just now.
        pub(super) dir: &'a Path,
        pub(super) cap: u64,
        /// Bytes copied plainly so far.
        pub(super) plain: u64,
        pub(super) clone: Clone,
        /// Learned from the first clone tried.
        pub(super) can_clone: Option<bool>,
        pub(super) out: &'a mut Included,
    }

    /// Why a path could not be opened, for people.
    fn why(e: Errno) -> String {
        match e {
            Errno::LOOP | Errno::NOTDIR => SYMLINK.to_owned(),
            Errno::NOENT => "gone before it was copied".to_owned(),
            Errno::EXIST => "already in the worktree".to_owned(),
            e => format!("cannot be copied: {e}"),
        }
    }

    /// The folder `path`'s parent is in, below `at`, and its last name;
    /// with `make`, missing folders on the way are made.
    pub(super) fn parent<'p>(
        at: &OwnedFd,
        path: &'p str,
        make: bool,
    ) -> Result<(OwnedFd, &'p str), Errno> {
        let (folders, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut fd = openat(at, ".", FOLDER, Mode::empty())?;
        for part in folders.split('/').filter(|p| !p.is_empty()) {
            if make {
                match mkdirat(&fd, part, Mode::from_raw_mode(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(e),
                }
            }
            fd = openat(&fd, part, FOLDER, Mode::empty())?;
        }
        Ok((fd, name))
    }

    impl Copy<'_> {
        pub(super) fn file(&mut self, path: &str) {
            let copied = parent(&self.from, path, false).and_then(|(from, name)| {
                let (to, _) = parent(&self.to, path, true)?;
                self.copy(&from, &to, name, path, false)
            });
            if let Err(e) = copied {
                self.out.skipped.push(skip(path, &why(e)));
            }
        }

        pub(super) fn folder(&mut self, path: &str) {
            if self.can_clone == Some(false) {
                self.out.skipped.push(skip(&format!("{path}/"), NO_CLONE));
                return;
            }
            let opened = parent(&self.from, path, false).and_then(|(from, name)| {
                let (to, _) = parent(&self.to, path, true)?;
                let from = openat(&from, name, FOLDER, Mode::empty())?;
                mkdirat(&to, name, Mode::from_raw_mode(0o755))?;
                Ok((from, openat(&to, name, FOLDER, Mode::empty())?))
            });
            let (from, to) = match opened {
                Ok(fds) => fds,
                Err(e) => return self.out.skipped.push(skip(&format!("{path}/"), &why(e))),
            };
            let before = self.out.copied.len();
            if self.walk(&from, &to, path).is_err() {
                // The first clone failed: nothing but empty folders was made.
                self.out.copied.truncate(before);
                let _ = std::fs::remove_dir_all(self.dir.join(path));
                self.out
                    .skipped
                    .retain(|s| !s.path.starts_with(&format!("{path}/")));
                self.out.skipped.push(skip(&format!("{path}/"), NO_CLONE));
            }
        }

        /// Clones everything in `from` into `to`; `Err` only when cloning
        /// turned out not to work at all.
        fn walk(&mut self, from: &OwnedFd, to: &OwnedFd, path: &str) -> Result<(), ()> {
            let Ok(mut entries) = Dir::read_from(from) else {
                self.out.skipped.push(skip(path, "cannot be read"));
                return Ok(());
            };
            let mut names = Vec::new();
            while let Some(Ok(entry)) = entries.read() {
                let name = entry.file_name().to_bytes();
                if name != b"." && name != b".." {
                    names.push(name.to_vec());
                }
            }
            names.sort();
            for name in names {
                let Ok(name) = String::from_utf8(name) else {
                    self.out
                        .skipped
                        .push(skip(path, "holds a name that is not UTF-8"));
                    continue;
                };
                let inner = format!("{path}/{name}");
                if name.eq_ignore_ascii_case(".git") {
                    self.out.skipped.push(skip(&inner, "under .git"));
                    continue;
                }
                let kind = rustix::fs::statat(from, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                    .map(|st| FileType::from_raw_mode(st.st_mode));
                let done = match kind {
                    Ok(FileType::Directory) => openat(from, name.as_str(), FOLDER, Mode::empty())
                        .and_then(|sub_from| {
                            mkdirat(to, name.as_str(), Mode::from_raw_mode(0o755))?;
                            let sub_to = openat(to, name.as_str(), FOLDER, Mode::empty())?;
                            Ok((sub_from, sub_to))
                        })
                        .map(|(sub_from, sub_to)| self.walk(&sub_from, &sub_to, &inner)),
                    Ok(_) => self.copy(from, to, &name, &inner, true).map(Ok),
                    Err(e) => Err(e),
                };
                match done {
                    Ok(Ok(())) => {}
                    Ok(Err(())) => return Err(()),
                    Err(Errno::NOTSUP) if self.can_clone == Some(false) => return Err(()),
                    Err(e) => self.out.skipped.push(skip(&inner, &why(e))),
                }
            }
            Ok(())
        }

        /// Copies the regular file `name` from `from` into `to` (`path`
        /// names it), cloned where it can be; `only_clone` for files in a
        /// folder. `NOTSUP` when it could only be copied plainly.
        fn copy(
            &mut self,
            from: &OwnedFd,
            to: &OwnedFd,
            name: &str,
            path: &str,
            only_clone: bool,
        ) -> Result<(), Errno> {
            let source = openat(from, name, SOURCE, Mode::empty())?;
            let stat = fstat(&source)?;
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
                // A link opened without following it fails with `LOOP`
                // already; this is a FIFO, socket or device.
                self.out.skipped.push(skip(path, "not a regular file"));
                return Ok(());
            }
            let mode = Mode::from_raw_mode(stat.st_mode & 0o777);
            let mut target = File::from(openat(to, name, TARGET, mode)?);
            let source = File::from(source);
            let bytes = u64::try_from(stat.st_size).unwrap_or(0);
            let cloned = self.can_clone != Some(false) && (self.clone)(&source, &target).is_ok();
            if self.can_clone.is_none() {
                self.can_clone = Some(cloned);
            }
            let unwritten = |reason: &str, out: &mut Included| {
                let _ = unlinkat(to, name, AtFlags::empty());
                out.skipped.push(skip(path, reason));
            };
            if !cloned {
                if only_clone {
                    let _ = unlinkat(to, name, AtFlags::empty());
                    if self.can_clone == Some(false) {
                        return Err(Errno::NOTSUP);
                    }
                    self.out.skipped.push(skip(path, "could not be cloned"));
                    return Ok(());
                }
                if self.plain + bytes > self.cap {
                    unwritten("over the 256 MiB for plain copies", self.out);
                    return Ok(());
                }
                if let Err(e) = io::copy(&mut &source, &mut &target) {
                    unwritten(&format!("cannot be copied: {e}"), self.out);
                    return Ok(());
                }
                self.plain += bytes;
            }
            match hash(&mut target).and_then(|hash| Ok((hash, target.metadata()?.len()))) {
                Ok((hash, bytes)) => self.out.copied.push(IncludedFile {
                    path: path.to_owned(),
                    bytes,
                    hash,
                }),
                Err(e) => unwritten(&format!("cannot be read back: {e}"), self.out),
            }
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs::{self, File};
    use std::io;
    use std::os::unix::fs::symlink;

    use ovra_core::services::Included;

    use super::{NO_CLONE, SYMLINK, copy_in};
    use crate::GitWorktrees;
    use crate::tests::{Repo, read};

    fn no_clone(_: &File, _: &File) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Stands in for a volume that clones.
    fn fake_clone(from: &File, to: &File) -> io::Result<()> {
        io::copy(&mut &*from, &mut &*to).map(|_| ())
    }

    fn skipped(included: &Included) -> Vec<(&str, &str)> {
        let skipped = included.skipped.iter();
        skipped
            .map(|s| (s.path.as_str(), s.reason.as_str()))
            .collect()
    }

    fn copied(included: &Included) -> Vec<(&str, u64)> {
        let copied = included.copied.iter();
        copied.map(|c| (c.path.as_str(), c.bytes)).collect()
    }

    /// A repository whose main checkout holds ignored files, and a worker's
    /// worktree of it.
    async fn setup(list: &str) -> (Repo, std::path::PathBuf) {
        let ignore = ".gitignore";
        let rules = "*.env\n*.local\nnode_modules/\n";
        let repo = Repo::new(&[
            (ignore, rules),
            (".worktreeinclude", list),
            ("config/app.toml", "a\n"),
        ]);
        let root = &repo.root;
        crate::tests::put(root, "config/db.local", "db\n");
        crate::tests::put(root, ".env", "TOKEN=1\n");
        crate::tests::put(root, "sub/.env", "SUB=1\n");
        crate::tests::put(root, "other.env", "no\n");
        crate::tests::put(root, "notes.log", "untracked, not ignored\n");
        crate::tests::put(root, "node_modules/pkg/index.js", "js\n");
        crate::tests::put(root, "node_modules/pkg/.git/config", "nested\n");
        crate::tests::put(repo.tmp.path(), "outside.txt", "outside\n");
        let outside = repo.tmp.path().join("outside.txt");
        symlink(&outside, root.join("link.env")).unwrap_or_else(|e| panic!("{e}"));
        symlink(&outside, root.join("node_modules/pkg/link.js")).unwrap_or_else(|e| panic!("{e}"));
        let w = GitWorktrees::new();
        let (_, wt) = repo.worker(&w).await;
        (repo, wt)
    }

    #[tokio::test]
    async fn only_listed_ignored_files_are_copied_and_never_through_a_link() {
        let list = ".env\n*.log\nlink.env\nnode_modules/\nconfig/\n";
        let (repo, wt) = setup(list).await;
        let included = copy_in(&repo.root, &wt, 1 << 20, no_clone);
        let included = included.unwrap_or_else(|e| panic!("{e:?}"));
        // `config/` is not ignored as a whole: only what is ignored in it.
        let files = [(".env", 8), ("config/db.local", 3), ("sub/.env", 6)];
        assert_eq!(copied(&included), files);
        assert_eq!(
            skipped(&included),
            [("link.env", SYMLINK), ("node_modules/", NO_CLONE)]
        );
        assert_eq!(read(&wt, ".env").as_deref(), Some("TOKEN=1\n"));
        assert!(!wt.join("other.env").exists());
        assert!(!wt.join("node_modules").exists());
        assert!(fs::symlink_metadata(wt.join("link.env")).is_err());
        let hash = &included.copied[0].hash;
        let mut file = File::open(wt.join(".env")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(super::hash(&mut file).ok().as_ref(), Some(hash));

        // Without the list nothing happens.
        fs::remove_file(repo.root.join(".worktreeinclude")).unwrap_or_else(|e| panic!("{e}"));
        let none = copy_in(&repo.root, &wt, 1 << 20, no_clone);
        assert_eq!(none.ok(), Some(Included::default()));
    }

    #[tokio::test]
    async fn folders_are_cloned_without_links_or_git_folders() {
        let (repo, wt) = setup("node_modules/\n/.env\n").await;
        // Clones do not count toward the cap for plain copies.
        let included = copy_in(&repo.root, &wt, 0, fake_clone);
        let included = included.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            copied(&included),
            [(".env", 8), ("node_modules/pkg/index.js", 3)]
        );
        assert_eq!(
            skipped(&included),
            [
                ("node_modules/pkg/.git", "under .git"),
                ("node_modules/pkg/link.js", SYMLINK)
            ]
        );
        assert_eq!(
            read(&wt, "node_modules/pkg/index.js").as_deref(),
            Some("js\n")
        );
        assert!(!wt.join("node_modules/pkg/.git").exists());
    }

    #[tokio::test]
    async fn plain_copies_stop_at_the_cap() {
        let (repo, wt) = setup(".env\nsub/.env\n").await;
        let included = copy_in(&repo.root, &wt, 10, no_clone);
        let included = included.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(copied(&included), [(".env", 8)]);
        let over = "over the 256 MiB for plain copies";
        assert_eq!(skipped(&included), [("sub/.env", over)]);
        assert!(!wt.join("sub/.env").exists());
    }

    #[test]
    fn a_folder_swapped_for_a_link_is_never_followed() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (top, real) = (tmp.path().join("top"), tmp.path().join("real"));
        fs::create_dir_all(&top).unwrap_or_else(|e| panic!("{e}"));
        crate::tests::put(&real, "x.env", "secret\n");
        symlink(&real, top.join("config")).unwrap_or_else(|e| panic!("{e}"));
        let top = rustix::fs::open(
            &top,
            rustix::fs::OFlags::DIRECTORY,
            rustix::fs::Mode::empty(),
        );
        let top = top.unwrap_or_else(|e| panic!("{e}"));
        let opened = super::unix::parent(&top, "config/x.env", false).map(|_| ());
        assert!(opened.is_err());
    }
}
