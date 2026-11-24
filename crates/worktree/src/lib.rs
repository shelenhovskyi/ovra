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
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Delta, Result, WorktreeInfo, Worktrees};

use crate::git::Git;

/// Keeps snapshots reachable, so `git gc` does not prune a baseline or the
/// base of a worktree while a session needs it; its reflog lists them all.
const SNAPSHOT_REF: &str = "refs/ovra/snapshot";

/// Why Ovra's worktrees are locked, as `git worktree list` shows it.
const LOCK_REASON: &str = "Ovra agent session";

/// Snapshots this young are kept by `prune_snapshots` whatever the core
/// says: a worker's snapshot is taken before its review item exists.
const KEEP_YOUNG: Duration = Duration::from_secs(10 * 60);

/// Worktrees through the `git` command line.
#[derive(Debug, Default)]
pub struct GitWorktrees {
    /// Held while the snapshot ref changes, so a prune never drops a
    /// snapshot taken while it rewrites the reflog.
    snapshot_ref: Arc<Mutex<()>>,
}

impl GitWorktrees {
    pub fn new() -> Self {
        Self::default()
    }
}

fn hold(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
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

fn snapshot(dir: &Path, snapshot_ref: &Mutex<()>) -> Result<String> {
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
    let _held = hold(snapshot_ref);
    log_snapshot(dir, &id)?;
    Ok(id)
}

fn log_snapshot(dir: &Path, id: &str) -> Result<()> {
    Git::new(dir, "update-ref")
        .args(["--create-reflog", "-m", "ovra: snapshot", SNAPSHOT_REF, id])
        .run()
        .map(|_| ())
}

/// Rewrites the snapshot reflog with only the snapshots in `keep` and the
/// young ones, oldest first; the rest become unreachable for `git gc`.
fn prune_snapshots(root: &Path, keep: &[String], snapshot_ref: &Mutex<()>) -> Result<()> {
    let _held = hold(snapshot_ref);
    // `<id> <committer time>`, newest first; nothing to do without the ref.
    let Some(log) = Git::new(root, "log")
        .args(["--walk-reflogs", "--format=%H %ct", SNAPSHOT_REF, "--"])
        .try_text()?
    else {
        return Ok(());
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let young_since = now.saturating_sub(KEEP_YOUNG.as_secs());
    let mut kept: Vec<&str> = Vec::new();
    let mut dropped = false;
    for line in log.lines() {
        let Some((id, time)) = line.split_once(' ') else {
            continue;
        };
        let young = time.parse::<u64>().is_ok_and(|t| t >= young_since);
        if (young || keep.iter().any(|k| k == id)) && !kept.contains(&id) {
            kept.push(id);
        } else {
            dropped = true;
        }
    }
    if !dropped {
        return Ok(());
    }
    // Deleting the ref deletes its reflog too.
    Git::new(root, "update-ref")
        .args(["-d", SNAPSHOT_REF])
        .run()?;
    for id in kept.iter().rev() {
        log_snapshot(root, id)?;
    }
    Ok(())
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
    // Locked for its whole life, so `git worktree prune` (also run by
    // `git gc`) keeps it while its folder is out of reach, and a plain
    // `git worktree remove` refuses it.
    Git::new(root, "worktree")
        .args([
            "add",
            "--quiet",
            "--lock",
            "--reason",
            LOCK_REASON,
            "-b",
            branch,
        ])
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
    // The session is over: its uncommitted files go, its branch stays. The
    // second `--force` removes the lock, also when the folder is gone.
    let removed = Git::new(root, "worktree")
        .args(["remove", "--force", "--force"])
        .arg(dir)
        .run();
    if dir.exists() {
        removed?;
    } else {
        // Unknown to git as well, or just removed: drop what is left.
        Git::new(root, "worktree").arg("prune").run()?;
    }
    Ok(())
}

impl Worktrees for GitWorktrees {
    fn snapshot(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        let snapshot_ref = Arc::clone(&self.snapshot_ref);
        blocking(move || snapshot(&dir, &snapshot_ref))
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

    fn prune_snapshots(&self, root: PathBuf, keep: Vec<String>) -> BoxFuture<'_, Result<()>> {
        let snapshot_ref = Arc::clone(&self.snapshot_ref);
        blocking(move || prune_snapshots(&root, &keep, &snapshot_ref))
    }
}
