//! Running `git` and temporary index files.
//!
//! Every command runs without a terminal prompt and without the `GIT_*`
//! variables a parent process may have set, so it only ever sees the
//! repository of its own directory and the real index unless it asks for a
//! [`TempIndex`].

use std::ffi::OsStr;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use ovra_core::path::RelPath;
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
        let mut command = Command::new("git");
        command
            .args(["--literal-pathspecs", "-c", "core.quotepath=off"])
            .arg(subcommand)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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

    /// `-- path...`
    pub(crate) fn paths<'a>(self, paths: impl IntoIterator<Item = &'a RelPath>) -> Self {
        self.arg("--").args(paths.into_iter().map(RelPath::as_str))
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
pub(crate) fn commit_id(dir: &Path, rev: &str) -> Result<String> {
    Git::new(dir, "rev-parse")
        .args(["--verify", "--quiet", "--end-of-options"])
        .arg(format!("{rev}^{{commit}}"))
        .try_text()?
        .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, format!("commit {rev} not found")))
}

/// The commit HEAD points at; `None` before the first commit.
pub(crate) fn head(dir: &Path) -> Result<Option<String>> {
    Git::new(dir, "rev-parse")
        .args(["--verify", "--quiet", "HEAD^{commit}"])
        .try_text()
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
        Ok(Self::at(&git_dir(dir)?.0))
    }

    /// A copy of the real index, so `git add` can reuse its file stats and
    /// only hashes files that changed.
    pub(crate) fn copy_of_real(dir: &Path) -> Result<Self> {
        let (git_dir, real) = git_dir(dir)?;
        let index = Self::at(&git_dir);
        match std::fs::copy(&real, &index.path) {
            Ok(_) => Ok(index),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(index),
            Err(e) => Err(internal(format!("cannot copy the index: {e}"))),
        }
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

/// The git directory of the worktree at `dir` and its real index file.
fn git_dir(dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let git_dir = PathBuf::from(
        Git::new(dir, "rev-parse")
            .arg("--absolute-git-dir")
            .text()?,
    );
    let index = git_dir.join("index");
    Ok((git_dir, index))
}

/// The tree of everything in the working tree of `dir`: tracked files as
/// they are on disk, untracked files that are not ignored, deletions.
pub(crate) fn working_tree(dir: &Path) -> Result<String> {
    let index = TempIndex::copy_of_real(dir)?;
    // Without a pathspec `add --all` covers the whole working tree.
    Git::new(dir, "add").arg("--all").index(&index).run()?;
    Git::new(dir, "write-tree").index(&index).text()
}
