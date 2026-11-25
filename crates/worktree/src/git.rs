//! Running `git` and temporary index files.
//!
//! Every command runs without a terminal prompt and without the `GIT_*`
//! variables a parent process may have set, so it only ever sees the
//! repository of its own directory and the real index unless it asks for a
//! [`TempIndex`].

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;

/// Variables that would point git at another repository, index or tree.
const INHERITED: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

pub(crate) fn internal(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, what.to_string())
}

/// The git to run. On Windows `git` on the PATH is usually a launcher that
/// starts the real `git.exe` as a second process; with dozens of git calls
/// per review, that doubles the cost, so the real one is used if it works.
pub(crate) fn program() -> &'static OsStr {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let launcher = PathBuf::from("git");
            if cfg!(windows) {
                real_git().unwrap_or(launcher)
            } else {
                launcher
            }
        })
        .as_os_str()
}

/// `<prefix>/bin/git.exe` next to its libraries, found from the exec path
/// `<prefix>/libexec/git-core`, if it runs.
fn real_git() -> Option<PathBuf> {
    let ok = |program: &OsStr, arg: &str| {
        Command::new(program)
            .arg(arg)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
    };
    let exec_path = ok(OsStr::new("git"), "--exec-path")?;
    let exec_path = PathBuf::from(String::from_utf8_lossy(&exec_path.stdout).trim());
    let real = exec_path.parent()?.parent()?.join("bin").join("git.exe");
    ok(real.as_os_str(), "--version").map(|_| real)
}

/// One `git` invocation in a directory.
#[derive(Debug)]
pub(crate) struct Git {
    command: Command,
    name: String,
    input: Option<Vec<u8>>,
}

impl Git {
    /// `git <subcommand>` in `dir`. Pathspecs are literal: file names with
    /// `*` or a leading `:` mean exactly that file.
    pub(crate) fn new(dir: &Path, subcommand: &str) -> Self {
        let mut command = Command::new(program());
        if let Some(path) = ovra_core::child_path::login() {
            command.env("PATH", path);
        }
        command
            .args(["--literal-pathspecs", "-c", "core.quotepath=off"])
            // Never run a configured file monitor, and refuse `.git` paths
            // in every spelling some disk treats as `.git`.
            .args(["-c", "core.fsmonitor=false"])
            .args(["-c", "core.protectNTFS=true", "-c", "core.protectHFS=true"])
            // Never look into or recurse into a submodule: an agent may have
            // changed one, and git would run that repository's config.
            .args(["-c", "diff.ignoreSubmodules=all"])
            .args(["-c", "submodule.recurse=false"])
            .arg(subcommand)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // `status` asks each submodule for its own status unless told not
        // to. `diff-tree` compares trees only, never a submodule's files,
        // but would hide a changed submodule commit when a `.gitmodules`
        // (which an agent can write) says `ignore = all`: a review must
        // always see it.
        match subcommand {
            "status" => {
                command.arg("--ignore-submodules=all");
            }
            "diff-tree" => {
                command.arg("--ignore-submodules=none");
            }
            _ => {}
        }
        for var in INHERITED {
            command.env_remove(var);
        }
        Self {
            command,
            name: subcommand.to_owned(),
            input: None,
        }
    }

    pub(crate) fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.command.arg(arg);
        self
    }

    pub(crate) fn args<S: AsRef<OsStr>>(mut self, args: impl IntoIterator<Item = S>) -> Self {
        self.command.args(args);
        self
    }

    pub(crate) fn env(mut self, key: &str, value: impl AsRef<OsStr>) -> Self {
        self.command.env(key, value);
        self
    }

    /// Uses `index` instead of the real index.
    pub(crate) fn index(self, index: &TempIndex) -> Self {
        self.env("GIT_INDEX_FILE", &index.path)
    }

    /// Feeds `input` on stdin.
    pub(crate) fn input(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.input = Some(input.into());
        self.command.stdin(Stdio::piped());
        self
    }

    /// Runs to the end; fails only if git could not be started.
    pub(crate) fn output(mut self) -> Result<Output> {
        let mut child = self
            .command
            .spawn()
            .map_err(|e| internal(format!("cannot run git: {e}")))?;
        if let (Some(input), Some(mut stdin)) = (self.input.take(), child.stdin.take()) {
            // A git that exits early closes the pipe; its status says why.
            let _ = stdin.write_all(&input);
        }
        child
            .wait_with_output()
            .map_err(|e| internal(format!("git {}: {e}", self.name)))
    }

    /// Stdout of a successful run; otherwise an error with git's message.
    pub(crate) fn run(self) -> Result<Vec<u8>> {
        let name = self.name.clone();
        let out = self.output()?;
        if out.status.success() {
            return Ok(out.stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(internal(format!("git {name}: {}", stderr.trim())))
    }

    /// Stdout of a successful run as trimmed text.
    pub(crate) fn text(self) -> Result<String> {
        let out = self.run()?;
        String::from_utf8(out)
            .map(|s| s.trim().to_owned())
            .map_err(|_| internal("git printed invalid UTF-8"))
    }

    /// Trimmed stdout if git succeeded, `None` if it exited with an error.
    pub(crate) fn try_text(self) -> Result<Option<String>> {
        let out = self.output()?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    }
}

/// Splits `-z` output into its NUL-terminated fields.
pub(crate) fn fields(out: &[u8]) -> impl Iterator<Item = String> + '_ {
    out.split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| String::from_utf8_lossy(f).into_owned())
}

/// The commit `rev` names in the repository of `dir`, or `NotFound`.
/// Commits never change: one that was found once is not asked again.
fn found() -> MutexGuard<'static, HashSet<(PathBuf, String)>> {
    static FOUND: OnceLock<Mutex<HashSet<(PathBuf, String)>>> = OnceLock::new();
    FOUND
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// `id` names a commit in the repository at `dir`, just written there.
pub(crate) fn known_commit(dir: &Path, id: &str) {
    found().insert((dir.to_path_buf(), id.to_owned()));
}

pub(crate) fn commit_id(dir: &Path, rev: &str) -> Result<String> {
    let key = (dir.to_path_buf(), rev.to_owned());
    if found().contains(&key) {
        return Ok(rev.to_owned());
    }
    let id = Git::new(dir, "rev-parse")
        .args(["--verify", "--quiet", "--end-of-options"])
        .arg(format!("{rev}^{{commit}}"))
        .try_text()?
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, format!("commit {rev} not found"))
        })?;
    if id == rev {
        found().insert(key);
    }
    Ok(id)
}

/// The commit HEAD points at; `None` before the first commit.
pub(crate) fn head(dir: &Path) -> Result<Option<String>> {
    if let Some(head) = head_from_files(&dirs(dir)?) {
        return Ok(head);
    }
    Git::new(dir, "rev-parse")
        .args(["--verify", "--quiet", "HEAD^{commit}"])
        .try_text()
}

/// HEAD read from the files git keeps it in, without starting git (a
/// snapshot needs it each time); `None` when git has to say.
fn head_from_files(dirs: &Dirs) -> Option<Option<String>> {
    let is_id = |s: &str| {
        matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    if dirs.common.join("reftable").exists() {
        return None;
    }
    let head = std::fs::read_to_string(dirs.git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(name) = head.strip_prefix("ref: ") else {
        return is_id(head).then(|| Some(head.to_owned()));
    };
    if !name.starts_with("refs/heads/") || name.split('/').any(|p| p.is_empty() || p == "..") {
        return None;
    }
    match std::fs::read_to_string(dirs.common.join(name)) {
        Ok(id) => return is_id(id.trim()).then(|| Some(id.trim().to_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let packed = match std::fs::read_to_string(dirs.common.join("packed-refs")) {
        Ok(packed) => packed,
        // A branch with no commit yet.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    let id = packed
        .lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(id, line_name)| *line_name == name && is_id(id))
        .map(|(id, _)| id.to_owned());
    Some(id)
}

static NEXT_INDEX: AtomicU64 = AtomicU64::new(0);

/// An index file of our own in the repository's git directory, deleted on
/// drop. The real index, HEAD and the stash never change through it.
#[derive(Debug)]
pub(crate) struct TempIndex {
    path: PathBuf,
}

impl TempIndex {
    /// An index that does not exist yet: git starts it empty.
    pub(crate) fn empty(dir: &Path) -> Result<Self> {
        Ok(Self::at(&dirs(dir)?.git_dir))
    }

    /// A copy of `source`, or an empty index if there is none. The copy
    /// keeps the source's modification time: git compares file times with
    /// it to spot files changed in the same second the index was written.
    fn copy_of(git_dir: &Path, source: &Path) -> Result<Self> {
        let index = Self::at(git_dir);
        let fail = |e: std::io::Error| internal(format!("cannot copy the index: {e}"));
        let modified = match std::fs::metadata(source).and_then(|m| m.modified()) {
            Ok(modified) => modified,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(index),
            Err(e) => return Err(fail(e)),
        };
        std::fs::copy(source, &index.path).map_err(fail)?;
        std::fs::File::options()
            .write(true)
            .open(&index.path)
            .and_then(|f| f.set_modified(modified))
            .map_err(fail)?;
        Ok(index)
    }

    /// Keeps the file at `to` instead of deleting it.
    fn keep_as(self, to: &Path) {
        // Losing a race with another snapshot only loses a cache.
        let _ = std::fs::rename(&self.path, to);
    }

    fn at(git_dir: &Path) -> Self {
        let n = NEXT_INDEX.fetch_add(1, Ordering::Relaxed);
        let name = format!("ovra-index-{}-{n}", std::process::id());
        Self {
            path: git_dir.join(name),
        }
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let mut lock = self.path.clone().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(lock);
    }
}

/// Where a working tree's repository files are.
#[derive(Debug, Clone)]
pub(crate) struct Dirs {
    /// The worktree's own git directory (index, HEAD, Ovra's cache).
    pub(crate) git_dir: PathBuf,
    /// The root of the working tree.
    pub(crate) top: PathBuf,
    /// The git directory all worktrees share (`info/exclude` is there).
    pub(crate) common: PathBuf,
    /// `dir` relative to `top`: `""` or ending in `/`.
    pub(crate) prefix: String,
}

fn known() -> MutexGuard<'static, HashMap<PathBuf, Dirs>> {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, Dirs>>> = OnceLock::new();
    KNOWN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The repository folders of `dir`, asked of git once per directory: every
/// git process costs tens of milliseconds on Windows. `NotFound` when `dir`
/// is not in a git repository.
pub(crate) fn dirs(dir: &Path) -> Result<Dirs> {
    if let Some(dirs) = known().get(dir).filter(|d| d.git_dir.is_dir()) {
        return Ok(dirs.clone());
    }
    // English messages, to tell "not a repository" from other failures;
    // `rev-parse` runs no hooks or filters the locale could matter to.
    let out = Git::new(dir, "rev-parse")
        .args(["--absolute-git-dir", "--show-toplevel"])
        .args([
            "--path-format=absolute",
            "--git-common-dir",
            "--show-prefix",
        ])
        .env("LC_ALL", "C")
        .output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("not a git repository") {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                "not a git repository",
            ));
        }
        return Err(internal(format!("git rev-parse: {}", stderr.trim())));
    }
    let out = String::from_utf8_lossy(&out.stdout);
    let mut lines = out.trim().lines();
    let (Some(git_dir), Some(top), Some(common)) = (lines.next(), lines.next(), lines.next())
    else {
        return Err(internal("git rev-parse printed too little"));
    };
    let dirs = Dirs {
        git_dir: PathBuf::from(git_dir),
        top: PathBuf::from(top),
        common: PathBuf::from(common),
        prefix: lines.next().unwrap_or_default().to_owned(),
    };
    known().insert(dir.to_path_buf(), dirs.clone());
    Ok(dirs)
}

/// Forgets what [`dirs`] knows of `dir`, for a worktree that goes away.
pub(crate) fn forget(dir: &Path) {
    known().remove(dir);
}

/// Ovra's own index of the working tree, in the git directory of each
/// worktree. Kept between snapshots so git's file stats cover untracked
/// files too, and only files changed since are hashed again.
const CACHE: &str = "ovra-index";

/// The tree of everything in the working tree of `dir`: tracked files as
/// they are on disk, untracked files that are not ignored, deletions.
pub(crate) fn working_tree(dir: &Path) -> Result<String> {
    let dirs = dirs(dir)?;
    let (git_dir, real) = (&dirs.git_dir, dirs.git_dir.join("index"));
    let cache = git_dir.join(CACHE);
    // The first time, the real index has the stats of the tracked files.
    let cached = std::fs::metadata(&cache).and_then(|m| m.modified()).ok();
    let index = TempIndex::copy_of(git_dir, if cached.is_some() { &cache } else { &real })?;
    // Without a pathspec `add --all` covers the whole working tree.
    let added = Git::new(dir, "add")
        .args(["--all", "--verbose"])
        .index(&index)
        .run()?;
    // Files this index once added can only have become ignored if an
    // ignore file changed since: one in the tree (`add` lists it) or
    // `info/exclude`. A global excludes file is not watched.
    let rules_changed = String::from_utf8_lossy(&added)
        .lines()
        .any(|line| line.contains(".gitignore"))
        || std::fs::metadata(dirs.common.join("info").join("exclude"))
            .and_then(|m| m.modified())
            .is_ok_and(|exclude| cached.is_some_and(|cached| exclude >= cached));
    if cached.is_some() && rules_changed {
        forget_ignored(dir, &index)?;
    }
    let tree = Git::new(dir, "write-tree").index(&index).text()?;
    index.keep_as(&cache);
    Ok(tree)
}

/// `add --all` keeps files it once added even after they are ignored. Drops
/// those, unless the real index tracks them.
fn forget_ignored(dir: &Path, index: &TempIndex) -> Result<()> {
    // From the top: `ls-files` only lists the folder it runs in.
    let top = dirs(dir)?.top;
    let ignored = |index: Option<&TempIndex>| -> Result<Vec<u8>> {
        let mut git =
            Git::new(&top, "ls-files").args(["-z", "--cached", "--ignored", "--exclude-standard"]);
        if let Some(index) = index {
            git = git.index(index);
        }
        git.run()
    };
    let ours = ignored(Some(index))?;
    if ours.is_empty() {
        return Ok(());
    }
    let tracked = ignored(None)?;
    let tracked: std::collections::BTreeSet<String> = fields(&tracked).collect();
    let mut stale = Vec::new();
    for path in fields(&ours).filter(|p| !tracked.contains(p)) {
        stale.extend_from_slice(path.as_bytes());
        stale.push(0);
    }
    if !stale.is_empty() {
        Git::new(&top, "update-index")
            .args(["-z", "--force-remove", "--stdin"])
            .index(index)
            .input(stale)
            .run()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_and_the_filters_it_runs_get_the_login_shells_path() {
        // Each test runs in its own process (nextest), so this stays here.
        // `git-lfs` from Homebrew is found only on the login shell's PATH.
        ovra_core::child_path::set_login("/opt/homebrew/bin:/usr/bin".into());
        let git = Git::new(Path::new("."), "status");
        let path = git
            .command
            .get_envs()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| value);
        assert_eq!(path, Some(OsStr::new("/opt/homebrew/bin:/usr/bin")));
    }
}
