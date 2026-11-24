//! Git worktrees for isolated sessions, working-tree snapshots for direct
//! mode (ADR 0005) and the diffs the review inbox shows.
//! Contract: `ovra_core::services::Worktrees`; fake:
//! `ovra_core::fakes::MemoryWorktrees`.
//!
//! Everything goes through the `git` command line. Reading the working tree
//! (snapshots, diffs) and restoring files use a temporary index, so HEAD,
//! the real index and the stash only change in `commit`.

mod diff;
#[cfg(test)]
mod direct;
mod files;
mod git;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Delta, Result, WorktreeInfo, Worktrees};

use crate::git::Git;

/// Keeps snapshots reachable, so `git gc` does not prune a baseline or the
/// base of a worktree while a session needs it; its reflog lists them all.
const SNAPSHOT_REF: &str = "refs/ovra/snapshot";

/// Worktrees through the `git` command line.
#[derive(Debug, Default)]
pub struct GitWorktrees {}

impl GitWorktrees {
    pub fn new() -> Self {
        Self {}
    }
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> BoxFuture<'static, Result<T>> {
    Box::pin(async move {
        tokio::task::spawn_blocking(work)
            .await
            .unwrap_or_else(|e| Err(git::internal(e)))
    })
}

fn snapshot(dir: &Path) -> Result<String> {
    let tree = git::working_tree(dir)?;
    let parent = git::head(dir)?;
    let mut commit = Git::new(dir, "commit-tree").arg("--no-gpg-sign").arg(&tree);
    if let Some(parent) = &parent {
        commit = commit.args(["-p", parent]);
    }
    // A fixed identity: snapshots are Ovra's, and work without a configured
    // user.
    let id = commit
        .args(["-m", "Ovra snapshot"])
        .env("GIT_AUTHOR_NAME", "Ovra")
        .env("GIT_AUTHOR_EMAIL", "ovra@localhost")
        .env("GIT_COMMITTER_NAME", "Ovra")
        .env("GIT_COMMITTER_EMAIL", "ovra@localhost")
        .text()?;
    Git::new(dir, "update-ref")
        .args(["--create-reflog", "-m", "ovra: snapshot", SNAPSHOT_REF, &id])
        .run()?;
    Ok(id)
}

fn create(root: &Path, dir: &Path, branch: &str, base: &str) -> Result<WorktreeInfo> {
    let conflict = |what: String| Err(ProtocolError::new(ErrorCode::Conflict, what));
    if dir.exists() {
        return conflict(format!("{} already exists", dir.display()));
    }
    let base = git::commit_id(root, base)?;
    let existing = Git::new(root, "rev-parse")
        .args(["--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .try_text()?;
    if existing.is_some() {
        return conflict(format!("branch {branch} already exists"));
    }
    Git::new(root, "worktree")
        .args(["add", "--quiet", "-b", branch])
        .arg(dir)
        .arg(&base)
        .run()?;
    Ok(WorktreeInfo {
        path: dir.to_path_buf(),
        branch: branch.to_owned(),
        base,
    })
}

fn remove(root: &Path, dir: &Path) -> Result<()> {
    if dir.exists() {
        // The session is over: its uncommitted files go, its branch stays.
        Git::new(root, "worktree")
            .args(["remove", "--force"])
            .arg(dir)
            .run()?;
    } else {
        Git::new(root, "worktree").arg("prune").run()?;
    }
    Ok(())
}

impl Worktrees for GitWorktrees {
    fn snapshot(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        blocking(move || snapshot(&dir))
    }

    fn create(
        &self,
        root: PathBuf,
        dir: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>> {
        blocking(move || create(&root, &dir, &branch, &base))
    }

    fn changes(&self, dir: PathBuf, base: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        blocking(move || diff::changes(&dir, &base))
    }

    fn patch(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<String>> {
        blocking(move || diff::patch(&dir, &base, &paths))
    }

    fn restore(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<()>> {
        blocking(move || files::restore(&dir, &base, &paths))
    }

    fn apply(
        &self,
        from: PathBuf,
        base: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>> {
        blocking(move || files::apply(&from, &base, &into, &paths))
    }

    fn commit(
        &self,
        dir: PathBuf,
        paths: Vec<RelPath>,
        message: String,
    ) -> BoxFuture<'_, Result<String>> {
        blocking(move || files::commit(&dir, &paths, &message))
    }

    fn remove(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>> {
        blocking(move || remove(&root, &dir))
    }

    /// Keeps every snapshot for now (task 3.A prunes the snapshot reflog).
    fn prune_snapshots(&self, _root: PathBuf, _keep: Vec<String>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
