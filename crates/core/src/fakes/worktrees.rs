//! [`MemoryWorktrees`]: worktrees and commits as maps of file texts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{lock, not_found};
use crate::path::RelPath;
use crate::protocol::review::ChangeKind;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{BoxFuture, Delta, GitFiles, Result, Taken, WorktreeInfo, Worktrees};

type Files = BTreeMap<String, String>;

/// Working trees keyed by directory, and commits keyed `c1`, `c2`, ...
/// Diffs count whole lines; a file moved with its exact text is a rename.
#[derive(Debug, Default)]
pub struct MemoryWorktrees {
    dirs: Mutex<BTreeMap<PathBuf, Files>>,
    commits: Mutex<Vec<Files>>,
    /// `(dir, paths, message)` of every `commit` call.
    pub committed: Mutex<Vec<(PathBuf, Vec<String>, String)>>,
    /// `(root, keep)` of every `prune_snapshots` call; commits stay.
    pub pruned: Mutex<Vec<(PathBuf, Vec<String>)>>,
    /// `(root, branch, base)` of every `delete_branch` call.
    pub deleted_branches: Mutex<Vec<(PathBuf, String, String)>>,
    /// When set, `remove` refuses with this reason, as the real one does
    /// for a worktree that holds ignored files.
    pub refuse_remove: Mutex<Option<String>>,
    /// What `git_files` reports, by directory; empty by default.
    pub git_files: Mutex<BTreeMap<PathBuf, GitFiles>>,
    /// When set, `snapshot` lets other tasks run first, as a real one that
    /// waits for git does.
    pub slow_snapshot: AtomicBool,
}

impl MemoryWorktrees {
    /// Writes a file in a working tree, creating the tree if needed.
    pub fn put(&self, dir: impl Into<PathBuf>, path: &str, text: &str) {
        lock(&self.dirs)
            .entry(dir.into())
            .or_default()
            .insert(path.to_owned(), text.to_owned());
    }

    pub fn delete(&self, dir: impl Into<PathBuf>, path: &str) {
        if let Some(files) = lock(&self.dirs).get_mut(&dir.into()) {
            files.remove(path);
        }
    }

    /// The files of a working tree; empty if there is none.
    pub fn files(&self, dir: impl Into<PathBuf>) -> Files {
        lock(&self.dirs)
            .get(&dir.into())
            .cloned()
            .unwrap_or_default()
    }

    /// Whether a working tree exists at `dir`.
    pub fn has(&self, dir: impl Into<PathBuf>) -> bool {
        lock(&self.dirs).contains_key(&dir.into())
    }

    fn tree(&self, dir: &PathBuf) -> Result<Files> {
        lock(&self.dirs)
            .get(dir)
            .cloned()
            .ok_or_else(|| not_found(format!("working tree {}", dir.display())))
    }

    fn commit_files(&self, id: &str) -> Result<Files> {
        let index = id
            .strip_prefix('c')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| n.checked_sub(1));
        index
            .and_then(|i| lock(&self.commits).get(i).cloned())
            .ok_or_else(|| not_found(format!("commit {id}")))
    }

    fn record(&self, files: Files) -> String {
        let mut commits = lock(&self.commits);
        commits.push(files);
        format!("c{}", commits.len())
    }
}

fn lines(text: Option<&String>) -> Vec<&str> {
    text.map(|t| t.lines().collect()).unwrap_or_default()
}

/// Every old line removed, every new line added.
fn unified(path: &str, old: Option<&String>, new: Option<&String>) -> String {
    let mut patch = format!("--- a/{path}\n+++ b/{path}\n");
    for line in lines(old) {
        patch.push_str(&format!("-{line}\n"));
    }
    for line in lines(new) {
        patch.push_str(&format!("+{line}\n"));
    }
    patch
}

/// What differs from `old` to `new`, by path. A deleted file whose exact
/// text was added under another path is a rename, as git would find it.
fn deltas(old: &Files, new: &Files) -> Vec<Delta> {
    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();
    let mut all: Vec<Delta> = paths
        .into_iter()
        .filter_map(|p| delta(p, old.get(p), new.get(p)))
        .collect();
    let gone: Vec<String> = all
        .iter()
        .filter(|d| d.kind == ChangeKind::Deleted)
        .map(|d| d.path.clone())
        .collect();
    for from in gone {
        let text = old.get(&from);
        let to = all
            .iter_mut()
            .find(|d| d.kind == ChangeKind::Added && d.from.is_none() && new.get(&d.path) == text);
        if let Some(to) = to {
            to.kind = ChangeKind::Renamed;
            to.from = Some(from.clone());
            (to.additions, to.deletions) = (0, 0);
            all.retain(|d| d.path != from);
        }
    }
    all
}

fn delta(path: &str, old: Option<&String>, new: Option<&String>) -> Option<Delta> {
    let kind = match (old, new) {
        (None, Some(_)) => ChangeKind::Added,
        (Some(_), None) => ChangeKind::Deleted,
        (Some(a), Some(b)) if a != b => ChangeKind::Modified,
        _ => return None,
    };
    let new_text = new;
    let (old, new) = (lines(old), lines(new));
    let count = |from: &[&str], other: &[&str]| {
        u32::try_from(from.iter().filter(|l| !other.contains(l)).count()).unwrap_or(u32::MAX)
    };
    Some(Delta {
        path: path.to_owned(),
        kind,
        from: None,
        additions: count(&new, &old),
        deletions: count(&old, &new),
        version: new_text.cloned().unwrap_or_default(),
    })
}

impl Worktrees for MemoryWorktrees {
    fn snapshot(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            if self.slow_snapshot.load(Ordering::Relaxed) {
                tokio::task::yield_now().await;
            }
            let files = self.tree(&dir)?;
            Ok(self.record(files))
        })
    }

    fn create(
        &self,
        _root: PathBuf,
        dir: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>> {
        Box::pin(async move {
            let files = self.commit_files(&base)?;
            let mut dirs = lock(&self.dirs);
            if dirs.contains_key(&dir) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("{} already exists", dir.display()),
                ));
            }
            dirs.insert(dir.clone(), files);
            Ok(WorktreeInfo {
                path: dir,
                branch,
                base,
            })
        })
    }

    fn changes(&self, dir: PathBuf, base: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move {
            let (old, new) = (self.commit_files(&base)?, self.tree(&dir)?);
            Ok(deltas(&old, &new))
        })
    }

    fn look(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        self.snapshot(dir)
    }

    fn git_files(&self, dir: PathBuf) -> BoxFuture<'_, Result<GitFiles>> {
        let files = lock(&self.git_files).get(&dir).cloned().unwrap_or_default();
        Box::pin(async move { Ok(files) })
    }

    fn patches(
        &self,
        _dir: PathBuf,
        from: String,
        to: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<BTreeMap<String, String>>> {
        Box::pin(async move {
            let (old, new) = (self.commit_files(&from)?, self.commit_files(&to)?);
            Ok(paths
                .into_iter()
                .map(|path| {
                    let key = path.as_str();
                    let patch = if old.get(key) == new.get(key) {
                        String::new()
                    } else {
                        unified(key, old.get(key), new.get(key))
                    };
                    (key.to_owned(), patch)
                })
                .collect())
        })
    }

    fn diff(&self, _dir: PathBuf, from: String, to: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move {
            let (old, new) = (self.commit_files(&from)?, self.commit_files(&to)?);
            Ok(deltas(&old, &new))
        })
    }

    fn patch(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let (old, new) = (self.commit_files(&base)?, self.tree(&dir)?);
            let mut patch = String::new();
            for path in paths {
                let key = path.as_str();
                patch.push_str(&unified(key, old.get(key), new.get(key)));
            }
            Ok(patch)
        })
    }

    fn restore(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let old = self.commit_files(&base)?;
            let mut dirs = lock(&self.dirs);
            let files = dirs
                .get_mut(&dir)
                .ok_or_else(|| not_found(format!("working tree {}", dir.display())))?;
            for path in paths {
                match old.get(path.as_str()) {
                    Some(text) => files.insert(path.as_str().to_owned(), text.clone()),
                    None => files.remove(path.as_str()),
                };
            }
            Ok(())
        })
    }

    fn apply(
        &self,
        tree: String,
        base: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async move {
            let (old, source) = (self.commit_files(&base)?, self.commit_files(&tree)?);
            let mut dirs = lock(&self.dirs);
            let target = dirs
                .get_mut(&into)
                .ok_or_else(|| not_found(format!("working tree {}", into.display())))?;
            let mut conflicts = Vec::new();
            for path in paths {
                let key = path.as_str();
                if target.get(key) != old.get(key) {
                    conflicts.push(key.to_owned());
                    continue;
                }
                match source.get(key) {
                    Some(text) => target.insert(key.to_owned(), text.clone()),
                    None => target.remove(key),
                };
            }
            Ok(conflicts)
        })
    }

    fn commit(
        &self,
        dir: PathBuf,
        paths: Vec<RelPath>,
        message: String,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let files = self.tree(&dir)?;
            let paths = paths.iter().map(|p| p.as_str().to_owned()).collect();
            lock(&self.committed).push((dir, paths, message));
            Ok(self.record(files))
        })
    }

    fn take(
        &self,
        tree: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Taken>> {
        Box::pin(async move {
            let (before, source) = (self.tree(&into)?, self.commit_files(&tree)?);
            let undo = self.record(before);
            let mut dirs = lock(&self.dirs);
            let target = dirs
                .get_mut(&into)
                .ok_or_else(|| not_found(format!("working tree {}", into.display())))?;
            for path in paths {
                let key = path.as_str();
                match source.get(key) {
                    Some(text) => target.insert(key.to_owned(), text.clone()),
                    None => target.remove(key),
                };
            }
            let after = self.record(target.clone());
            Ok(Taken {
                undo,
                after,
                skipped: Vec::new(),
            })
        })
    }

    fn delete_branch(
        &self,
        root: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            lock(&self.deleted_branches).push((root, branch, base));
            Ok(())
        })
    }

    fn prune_snapshots(&self, root: PathBuf, keep: Vec<String>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            lock(&self.pruned).push((root, keep));
            Ok(())
        })
    }

    fn remove(&self, _root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if let Some(reason) = lock(&self.refuse_remove).clone() {
                return Err(crate::protocol::ProtocolError::new(
                    crate::protocol::ErrorCode::Conflict,
                    reason,
                ));
            }
            lock(&self.dirs)
                .remove(&dir)
                .map(|_| ())
                .ok_or_else(|| not_found(format!("working tree {}", dir.display())))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(p: &str) -> RelPath {
        RelPath::parse(p).unwrap_or_else(|e| panic!("{e}"))
    }

    #[tokio::test]
    async fn a_worktree_diffs_restores_and_applies_against_its_base() {
        let w = MemoryWorktrees::default();
        w.put("/main", "a.rs", "one\ntwo\n");
        w.put("/main", "b.rs", "keep\n");
        let base = w.snapshot("/main".into()).await.unwrap_or_default();
        let wt = w
            .create(
                "/main".into(),
                "/wt".into(),
                "ovra/t1/x".into(),
                base.clone(),
            )
            .await
            .map(|info| info.path);
        assert_eq!(wt.ok(), Some(PathBuf::from("/wt")));

        w.put("/wt", "a.rs", "one\n2\n");
        w.put("/wt", "c.rs", "new\n");
        w.delete("/wt", "b.rs");
        let changes = w
            .changes("/wt".into(), base.clone())
            .await
            .unwrap_or_default();
        let summary: Vec<_> = changes
            .iter()
            .map(|d| (d.path.as_str(), d.kind, d.additions, d.deletions))
            .collect();
        assert_eq!(
            summary,
            [
                ("a.rs", ChangeKind::Modified, 1, 1),
                ("b.rs", ChangeKind::Deleted, 0, 1),
                ("c.rs", ChangeKind::Added, 1, 0)
            ]
        );

        // The person edits b.rs in the main checkout meanwhile: a conflict.
        w.put("/main", "b.rs", "mine\n");
        let paths = vec![rel("a.rs"), rel("b.rs"), rel("c.rs")];
        let tree = w.snapshot("/wt".into()).await.unwrap_or_default();
        let conflicts = w
            .apply(tree, base.clone(), "/main".into(), paths)
            .await
            .unwrap_or_default();
        assert_eq!(conflicts, ["b.rs"]);
        assert_eq!(
            w.files("/main").get("a.rs").map(String::as_str),
            Some("one\n2\n")
        );
        assert_eq!(
            w.files("/main").get("b.rs").map(String::as_str),
            Some("mine\n")
        );

        let restored = w.restore("/wt".into(), base.clone(), vec![rel("a.rs"), rel("c.rs")]);
        assert!(restored.await.is_ok());
        assert_eq!(w.files("/wt").keys().collect::<Vec<_>>(), ["a.rs"]);
        assert!(w.remove("/main".into(), "/wt".into()).await.is_ok());
    }
}
