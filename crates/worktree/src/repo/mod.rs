//! The git panel's operations on a project's main checkout
//! (`docs/specs/git-panel.md`). Contract: `ovra_core::services::GitService`;
//! fake: `ovra_core::fakes::MemoryGit`.
//!
//! Paths come in relative to the project, which may be a folder inside the
//! repository: git runs in the project folder, so pathspecs mean the same
//! there, and paths git prints from the top are translated back.

mod status;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;

use ovra_core::path::RelPath;
use ovra_core::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, MAX_GIT_FILE,
    RepoStatus,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, GitService, Result};

use crate::blocking;
use crate::files::{self, OnDisk};
use crate::git::{self, Git, internal};

/// The most bytes of git's and the hooks' output passed on.
const MAX_OUTPUT: usize = 64 * 1024;

/// Pathspecs on stdin, NUL-separated: never on the command line.
const PATHSPEC_STDIN: [&str; 2] = ["--pathspec-from-file=-", "--pathspec-file-nul"];

/// `--format` of one commit: id, author, author time, summary.
const COMMIT_FORMAT: &str = "--format=%H%x00%an%x00%at%x00%s";

/// The git panel through the `git` command line.
#[derive(Debug, Default)]
pub struct GitRepo;

impl GitRepo {
    pub fn new() -> Self {
        Self
    }
}

fn conflict(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}

fn bad(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// `text` cut to at most `max` bytes, on a character boundary.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// What git and its hooks printed, trimmed: stderr (where hooks print)
/// first, then stdout.
fn printed(out: &Output) -> String {
    let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    cut(text.trim(), MAX_OUTPUT)
}

fn pathspecs(paths: &[RelPath]) -> Vec<u8> {
    let list: Vec<&[u8]> = paths.iter().map(|p| p.as_str().as_bytes()).collect();
    list.join(&0)
}

/// `id\0author\0time\0summary`, as [`COMMIT_FORMAT`] prints it.
fn commit_info(line: &str) -> Option<CommitInfo> {
    let mut parts = line.splitn(4, '\0');
    let (id, author, time) = (parts.next()?, parts.next()?, parts.next()?);
    Some(CommitInfo {
        id: id.to_owned(),
        author: author.to_owned(),
        time: time.parse().ok()?,
        summary: parts.next().unwrap_or_default().to_owned(),
    })
}

fn stage(root: &Path, paths: &[RelPath]) -> Result<()> {
    git::dirs(root)?;
    Git::new(root, "add")
        .arg("--all")
        .args(PATHSPEC_STDIN)
        .input(pathspecs(paths))
        .run()
        .map(drop)
}

/// `git reset` of paths puts HEAD's version in the index, and works before
/// the first commit too (it then empties those index entries).
fn unstage(root: &Path, paths: &[RelPath]) -> Result<()> {
    git::dirs(root)?;
    Git::new(root, "reset")
        .arg("--quiet")
        .args(PATHSPEC_STDIN)
        .input(pathspecs(paths))
        .run()
        .map(drop)
}

fn discard(root: &Path, paths: &[RelPath]) -> Result<()> {
    git::dirs(root)?;
    // The index entries of the project, relative to it.
    let listed = Git::new(root, "ls-files").args(["-z", "--cached"]).run()?;
    let tracked: BTreeSet<String> = git::fields(&listed).collect();
    let is_tracked = |path: &str| {
        let folder = format!("{path}/");
        tracked.contains(path)
            || tracked
                .range(folder.clone()..)
                .next()
                .is_some_and(|t| t.starts_with(&folder))
    };
    let (known, untracked): (Vec<RelPath>, Vec<RelPath>) =
        paths.iter().cloned().partition(|p| is_tracked(p.as_str()));
    if !known.is_empty() {
        Git::new(root, "restore")
            .arg("--worktree")
            .args(PATHSPEC_STDIN)
            .input(pathspecs(&known))
            .run()?;
    }
    // Folders, and files under a symlinked folder, are left alone.
    for path in &untracked {
        if files::on_disk(root, path)? == OnDisk::File {
            files::remove_file(root, path)?;
        }
    }
    Ok(())
}

fn last_commit(root: &Path) -> Result<CommitInfo> {
    let out = Git::new(root, "log")
        .args(["-1", "--no-show-signature", COMMIT_FORMAT, "HEAD", "--"])
        .text()?;
    commit_info(&out).ok_or_else(|| internal("git log printed no commit"))
}

fn commit(root: &Path, message: &str, amend: bool) -> Result<Committed> {
    let dirs = git::dirs(root)?;
    // A merge may be committed as it is; anything else needs staged changes.
    if !amend && !dirs.git_dir.join("MERGE_HEAD").exists() {
        let same = Git::new(root, "diff")
            .args(["--cached", "--quiet", "--no-ext-diff", "--no-textconv"])
            .arg("--ignore-submodules=none")
            .output()?;
        match same.status.code() {
            Some(0) => return Err(bad("nothing is staged to commit")),
            Some(1) => {}
            _ => return Err(internal(format!("git diff: {}", printed(&same)))),
        }
    }
    let mut run = Git::new(root, "commit").args(["-F", "-"]);
    if amend {
        run = run.arg("--amend");
    }
    let out = run.input(message).output()?;
    let output = printed(&out);
    if !out.status.success() {
        return Err(conflict(if output.is_empty() {
            "git commit failed without saying why".to_owned()
        } else {
            format!("git commit failed:\n{output}")
        }));
    }
    Ok(Committed {
        commit: last_commit(root)?,
        output,
    })
}

/// Whether `bytes` look like a binary file, as git judges it.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

fn show(root: &Path, path: &RelPath, rev: GitRev) -> Result<GitFileText> {
    let dirs = git::dirs(root)?;
    let at = match rev {
        GitRev::Head => "HEAD:",
        GitRev::Index => ":0:",
    };
    // The name on stdin, from the top of the repository.
    let out = Git::new(root, "cat-file")
        .arg("--batch-check")
        .input(format!("{at}{}{path}\n", dirs.prefix))
        .text()?;
    let mut parts = out.split(' ');
    let (Some(id), Some("blob"), Some(size), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Ok(GitFileText {
            text: None,
            exists: false,
        });
    };
    let size: u64 = size
        .parse()
        .map_err(|_| internal("git printed a bad size"))?;
    if size > MAX_GIT_FILE {
        return Ok(GitFileText {
            text: None,
            exists: true,
        });
    }
    let bytes = Git::new(root, "cat-file").args(["blob", id]).run()?;
    let text = (!is_binary(&bytes))
        .then(|| String::from_utf8(bytes).ok())
        .flatten();
    Ok(GitFileText { text, exists: true })
}

fn log(root: &Path, limit: u32) -> Result<Vec<CommitInfo>> {
    git::dirs(root)?;
    if git::head(root)?.is_none() {
        return Ok(Vec::new());
    }
    let out = Git::new(root, "log")
        .arg(format!("--max-count={limit}"))
        .args(["--no-show-signature", COMMIT_FORMAT, "HEAD", "--"])
        .run()?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(commit_info)
        .collect())
}

fn later<T: Send + 'static>() -> BoxFuture<'static, Result<T>> {
    Box::pin(async {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "this part of the git panel is not implemented yet",
        ))
    })
}

impl GitService for GitRepo {
    fn status(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        blocking(move || status::status(&root))
    }
    fn stage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(move || stage(&root, &paths))
    }
    fn unstage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(move || unstage(&root, &paths))
    }
    fn stage_hunk(&self, _: PathBuf, _: RelPath, _: Hunk, _: bool) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn discard(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(move || discard(&root, &paths))
    }
    fn commit(
        &self,
        root: PathBuf,
        message: String,
        amend: bool,
    ) -> BoxFuture<'_, Result<Committed>> {
        blocking(move || commit(&root, &message, amend))
    }
    fn branches(&self, _: PathBuf) -> BoxFuture<'_, Result<Vec<BranchInfo>>> {
        later()
    }
    fn switch(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn create_branch(&self, _: PathBuf, _: String, _: bool) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn diff(&self, _: PathBuf, _: RelPath, _: bool) -> BoxFuture<'_, Result<FileDiff>> {
        later()
    }
    fn show(
        &self,
        root: PathBuf,
        path: RelPath,
        rev: GitRev,
    ) -> BoxFuture<'_, Result<GitFileText>> {
        blocking(move || show(&root, &path, rev))
    }
    fn blame(&self, _: PathBuf, _: RelPath) -> BoxFuture<'_, Result<Blame>> {
        later()
    }
    fn log(&self, root: PathBuf, limit: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>> {
        blocking(move || log(&root, limit))
    }
}
