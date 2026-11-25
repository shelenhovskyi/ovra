//! The git panel's operations on a project's main checkout
//! (`docs/specs/git-panel.md`). Contract: `ovra_core::services::GitService`;
//! fake: `ovra_core::fakes::MemoryGit`.
//!
//! Paths come in relative to the project, which may be a folder inside the
//! repository: git runs in the project folder, so pathspecs mean the same
//! there, and paths git prints from the top are translated back.

mod blame;
mod diff;
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

/// `git init` in `root`, only after asking git whether `root` is in a
/// repository already: `rev-parse` looks at `root` and its parents (and
/// stops at a mount point, as git itself does), never at the folder's files.
fn init(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "the project folder does not exist",
        ));
    }
    // A `.git` git cannot read (a broken file or folder) is still someone's.
    if root.join(".git").symlink_metadata().is_ok() {
        return Err(conflict("this folder already has a .git"));
    }
    match git::dirs(root) {
        Ok(dirs) => Err(conflict(format!(
            "this folder is already in the git repository at {}",
            dirs.top.display()
        ))),
        Err(e) if e.code == ErrorCode::NotFound => {
            Git::new(root, "init").arg("--quiet").run().map(drop)
        }
        Err(e) => Err(e),
    }
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
    let listed = Git::new(root, "ls-files")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-z", "--cached"])
        .run()?;
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
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["--cached", "--quiet", "--no-ext-diff", "--no-textconv"])
            .arg("--ignore-submodules=none")
            .output()?;
        match same.status.code() {
            Some(0) => return Err(bad("nothing is staged to commit")),
            Some(1) => {}
            _ => return Err(internal(format!("git diff: {}", printed(&same)))),
        }
    }
    let keep = amend && message.trim().is_empty();
    let mut run = Git::new(root, "commit");
    run = if keep {
        // Amend what is staged into the last commit, message and all.
        run.args(["--amend", "--no-edit"])
    } else if amend {
        run.args(["-F", "-", "--amend"]).input(message)
    } else {
        run.args(["-F", "-"]).input(message)
    };
    let out = run.output()?;
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

fn branches(root: &Path) -> Result<Vec<BranchInfo>> {
    git::dirs(root)?;
    let out = Git::new(root, "for-each-ref")
        .args(["--sort=-committerdate", "--no-color"])
        .arg(
            "--format=%(refname)%00%(HEAD)%00%(upstream:short)%00\
             %(objectname)%00%(authorname)%00%(authordate:unix)%00%(contents:subject)",
        )
        .arg("refs/heads")
        .run()?;
    let out = String::from_utf8_lossy(&out);
    let mut list = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(4, '\0');
        let (Some(name), Some(head), Some(upstream), Some(commit)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/heads/") else {
            continue;
        };
        list.push(BranchInfo {
            name: name.to_owned(),
            current: head == "*",
            upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
            commit: commit_info(commit),
        });
    }
    // A branch without commits has no ref yet; HEAD still names it.
    if !list.iter().any(|b| b.current) {
        let head = Git::new(root, "symbolic-ref")
            .args(["--quiet", "HEAD"])
            .try_text()?;
        if let Some(name) = head.as_deref().and_then(|h| h.strip_prefix("refs/heads/")) {
            list.insert(
                0,
                BranchInfo {
                    name: name.to_owned(),
                    current: true,
                    upstream: None,
                    commit: None,
                },
            );
        }
    }
    Ok(list)
}

/// The branch name git makes of `name`, or `BadRequest`.
fn branch_name(root: &Path, name: &str) -> Result<String> {
    Git::new(root, "check-ref-format")
        .args(["--branch", name])
        .try_text()?
        .filter(|n| !n.is_empty())
        .ok_or_else(|| bad(format!("{name:?} is not a valid branch name")))
}

fn has_branch(root: &Path, name: &str) -> Result<bool> {
    Ok(Git::new(root, "rev-parse")
        .args(["--verify", "--quiet"])
        .arg(format!("refs/heads/{name}"))
        .try_text()?
        .is_some())
}

fn switch(root: &Path, name: &str) -> Result<()> {
    git::dirs(root)?;
    let name = branch_name(root, name)?;
    if !has_branch(root, &name)? {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            format!("there is no branch {name}"),
        ));
    }
    // Never forced: git refuses when changes would be overwritten.
    let out = Git::new(root, "switch")
        .args(["--quiet", "--no-guess", "--", &name])
        .output()?;
    if !out.status.success() {
        return Err(conflict(refused_switch(&printed(&out))));
    }
    Ok(())
}

/// Files named in a refused switch before "and N more".
const NAMED_FILES: usize = 5;

/// Why git refused a switch, for the person: what to do in Ovra when it
/// would overwrite changes (git's text suggests stash, which Ovra does not
/// offer), git's own text otherwise. The caller names the branch.
fn refused_switch(printed: &str) -> String {
    if !printed.contains("would be overwritten") {
        return printed.to_owned();
    }
    // git lists the files one per line, each indented by a tab.
    let files: Vec<&str> = printed
        .lines()
        .filter(|l| l.starts_with('\t'))
        .map(str::trim)
        .collect();
    let named = match files.len() {
        0 => String::new(),
        1 => format!(" to {}", files[0]),
        n if n <= NAMED_FILES => format!(" to {} and {}", files[..n - 1].join(", "), files[n - 1]),
        n => format!(
            " to {} and {} more files",
            files[..NAMED_FILES].join(", "),
            n - NAMED_FILES
        ),
    };
    format!(
        "you have changes{named} that this branch would overwrite: \
         commit them or discard them first"
    )
}

fn create_branch(root: &Path, name: &str, switch: bool) -> Result<()> {
    git::dirs(root)?;
    let name = branch_name(root, name)?;
    if has_branch(root, &name)? {
        return Err(conflict(format!("a branch named {name} already exists")));
    }
    if switch {
        // Also before the first commit: HEAD then names the new branch.
        Git::new(root, "switch")
            .args(["--quiet", "-c", &name])
            .run()?;
    } else {
        if git::head(root)?.is_none() {
            return Err(bad("a branch needs a commit to start from; commit first"));
        }
        Git::new(root, "branch").args(["--", &name]).run()?;
    }
    Ok(())
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

impl GitService for GitRepo {
    fn status(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        blocking(move || status::status(&root))
    }
    fn init(&self, root: PathBuf) -> BoxFuture<'_, Result<()>> {
        blocking(move || init(&root))
    }
    fn stage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(move || stage(&root, &paths))
    }
    fn unstage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        blocking(move || unstage(&root, &paths))
    }
    fn stage_hunk(
        &self,
        root: PathBuf,
        path: RelPath,
        hunk: Hunk,
        unstage: bool,
    ) -> BoxFuture<'_, Result<()>> {
        blocking(move || diff::stage_hunk(&root, &path, &hunk, unstage))
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
    fn branches(&self, root: PathBuf) -> BoxFuture<'_, Result<Vec<BranchInfo>>> {
        blocking(move || branches(&root))
    }
    fn switch(&self, root: PathBuf, branch: String) -> BoxFuture<'_, Result<()>> {
        blocking(move || switch(&root, &branch))
    }
    fn cancel(&self, _: PathBuf) -> BoxFuture<'_, Result<()>> {
        // Commits and switches cannot be stopped yet.
        Box::pin(async { Ok(()) })
    }
    fn create_branch(
        &self,
        root: PathBuf,
        name: String,
        switch: bool,
    ) -> BoxFuture<'_, Result<()>> {
        blocking(move || create_branch(&root, &name, switch))
    }
    fn diff(&self, root: PathBuf, path: RelPath, staged: bool) -> BoxFuture<'_, Result<FileDiff>> {
        blocking(move || diff::diff(&root, &path, staged))
    }
    fn show(
        &self,
        root: PathBuf,
        path: RelPath,
        rev: GitRev,
    ) -> BoxFuture<'_, Result<GitFileText>> {
        blocking(move || show(&root, &path, rev))
    }
    fn blame(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<Blame>> {
        blocking(move || blame::blame(&root, &path))
    }
    fn log(&self, root: PathBuf, limit: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>> {
        blocking(move || log(&root, limit))
    }
}
