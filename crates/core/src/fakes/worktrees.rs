//! [`MemoryWorktrees`]: worktrees and commits as maps of file texts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{lock, not_found};
use crate::path::RelPath;
use crate::protocol::git::Hunk;
use crate::protocol::orchestration::DiskUsage;
use crate::protocol::review::ChangeKind;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    BoxFuture, CommitAuthor, Delta, GitFiles, Included, IncludedFile, Merged, Picked, Result,
    Taken, WorktreeInfo, Worktrees,
};

type Files = BTreeMap<String, String>;

/// Working trees keyed by directory, and commits keyed `c1`, `c2`, ...
/// Diffs count whole lines; a file moved with its exact text is a rename.
/// A delta's version is the file's text, or git's for an empty file (as
/// the sandbox's placeholders are).
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
    /// `(dir, included)` of every `remove` call that removed a tree.
    pub removed: Mutex<Vec<(PathBuf, Vec<IncludedFile>)>>,
    /// What `include` reports. Each copied path the main checkout has is
    /// copied into the worktree with its text.
    pub to_include: Mutex<Included>,
    /// Locked worktrees and their reasons.
    pub locked: Mutex<BTreeMap<PathBuf, String>>,
    /// What `free_space` reports; `None` answers unavailable.
    pub free: Mutex<Option<u64>>,
    /// What `git_files` reports, by directory; empty by default.
    pub git_files: Mutex<BTreeMap<PathBuf, GitFiles>>,
    /// When set, `snapshot` lets other tasks run first, as a real one that
    /// waits for git does.
    pub slow_snapshot: AtomicBool,
    /// When set, the next `create` waits until its sender sends or drops.
    pub create_gate: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
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
    let (old_lines, new_lines) = (lines(old).len(), lines(new).len());
    let mut patch = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{},{old_lines} +{},{new_lines} @@\n",
        old_lines.min(1),
        new_lines.min(1)
    );
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
        version: match new_text {
            Some(text) if text.is_empty() => {
                "100644 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".to_owned()
            }
            text => text.cloned().unwrap_or_default(),
        },
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
            let gate = lock(&self.create_gate).take();
            if let Some(gate) = gate {
                let _ = gate.await;
            }
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

    /// Like git's tree of the working tree: the same files, the same id.
    fn look(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            if self.slow_snapshot.load(Ordering::Relaxed) {
                tokio::task::yield_now().await;
            }
            let files = self.tree(&dir)?;
            let seen = lock(&self.commits).iter().rposition(|c| *c == files);
            Ok(match seen {
                Some(i) => format!("c{}", i + 1),
                None => self.record(files),
            })
        })
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

    fn revert_hunk(&self, dir: PathBuf, path: RelPath, hunk: Hunk) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            hunk.check()?;
            let mut dirs = lock(&self.dirs);
            let files = dirs
                .get_mut(&dir)
                .ok_or_else(|| not_found(format!("working tree {}", dir.display())))?;
            let gone = || {
                ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("the change no longer applies to {path}"),
                )
            };
            let text = files.get(path.as_str()).ok_or_else(gone)?;
            let side = |keep: char| -> Vec<&str> {
                hunk.lines
                    .iter()
                    .filter(|l| l.starts_with(' ') || l.starts_with(keep))
                    .map(|l| &l[1..])
                    .collect()
            };
            let (old, new) = (side('-'), side('+'));
            let mut lines: Vec<&str> = text.lines().collect();
            let at = usize::try_from(hunk.new_start.saturating_sub(1)).unwrap_or(usize::MAX);
            // Like git: a hunk with nothing on its old side is the whole file.
            let whole = hunk.old_lines == 0 && lines.len() != new.len();
            if whole || lines.get(at..at + new.len()) != Some(&new[..]) {
                return Err(gone());
            }
            lines.splice(at..at + new.len(), old);
            let mut out = lines.join("\n");
            if !out.is_empty() && (text.ends_with('\n') || text.is_empty()) {
                out.push('\n');
            }
            files.insert(path.as_str().to_owned(), out);
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
                // Already as in `tree`: nothing to copy, no conflict.
                if target.get(key) == source.get(key) {
                    continue;
                }
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

    fn remove(
        &self,
        _root: PathBuf,
        dir: PathBuf,
        included: Vec<IncludedFile>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if let Some(reason) = lock(&self.refuse_remove).clone() {
                return Err(crate::protocol::ProtocolError::new(
                    crate::protocol::ErrorCode::Conflict,
                    reason,
                ));
            }
            lock(&self.dirs)
                .remove(&dir)
                .ok_or_else(|| not_found(format!("working tree {}", dir.display())))?;
            lock(&self.locked).remove(&dir);
            lock(&self.removed).push((dir, included));
            Ok(())
        })
    }

    fn pick(
        &self,
        _root: PathBuf,
        base: String,
        start: String,
        reviewed: String,
        _message: String,
        _author: Option<CommitAuthor>,
    ) -> BoxFuture<'_, Result<Picked>> {
        Box::pin(async move {
            let (mut tree, start, reviewed) = (
                self.commit_files(&base)?,
                self.commit_files(&start)?,
                self.commit_files(&reviewed)?,
            );
            let ours = tree.clone();
            let mut conflicts = Vec::new();
            let paths: std::collections::BTreeSet<&String> =
                start.keys().chain(reviewed.keys()).collect();
            for path in paths {
                let (was, now, there) = (start.get(path), reviewed.get(path), ours.get(path));
                if was == now || there == now {
                    continue;
                }
                if there != was {
                    conflicts.push(path.clone());
                    continue;
                }
                match now {
                    Some(text) => tree.insert(path.clone(), text.clone()),
                    None => tree.remove(path),
                };
            }
            if conflicts.is_empty() {
                Ok(Picked::Commit(self.record(tree)))
            } else {
                Ok(Picked::Conflicts(conflicts))
            }
        })
    }

    /// Whole files: one changed on both sides, differently, holds both
    /// versions between markers.
    fn merge(
        &self,
        _root: PathBuf,
        base: String,
        mine: String,
        main: String,
    ) -> BoxFuture<'_, Result<Merged>> {
        Box::pin(async move {
            let (was, ours, theirs) = (
                self.commit_files(&base)?,
                self.commit_files(&mine)?,
                self.commit_files(&main)?,
            );
            let mut tree = ours.clone();
            let mut conflicts = Vec::new();
            let paths: std::collections::BTreeSet<&String> =
                was.keys().chain(theirs.keys()).collect();
            for path in paths {
                let (before, here, there) = (was.get(path), ours.get(path), theirs.get(path));
                if before == there || here == there {
                    continue;
                }
                if here != before {
                    let side = |t: Option<&String>| t.cloned().unwrap_or_default();
                    let text = format!(
                        "<<<<<<< mine\n{}=======\n{}>>>>>>> main\n",
                        side(here),
                        side(there)
                    );
                    tree.insert(path.clone(), text);
                    conflicts.push(path.clone());
                    continue;
                }
                match there {
                    Some(text) => tree.insert(path.clone(), text.clone()),
                    None => tree.remove(path),
                };
            }
            // A file one side deleted cannot hold markers.
            let whole = conflicts
                .iter()
                .filter(|p| !ours.contains_key(*p) || !theirs.contains_key(*p))
                .cloned()
                .collect();
            Ok(Merged {
                commit: self.record(tree),
                conflicts,
                whole,
            })
        })
    }

    fn include(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<Included>> {
        Box::pin(async move {
            let included = lock(&self.to_include).clone();
            let main = self.tree(&root)?;
            let mut dirs = lock(&self.dirs);
            let tree = dirs
                .get_mut(&dir)
                .ok_or_else(|| not_found(format!("working tree {}", dir.display())))?;
            for file in &included.copied {
                if let Some(text) = main.get(&file.path) {
                    tree.insert(file.path.clone(), text.clone());
                }
            }
            Ok(included)
        })
    }

    fn usage(&self, dir: PathBuf, _budget: Duration) -> BoxFuture<'_, Result<DiskUsage>> {
        Box::pin(async move {
            let bytes = self.tree(&dir)?.values().map(|t| t.len() as u64).sum();
            Ok(DiskUsage {
                bytes,
                complete: true,
            })
        })
    }

    fn lock(&self, _root: PathBuf, dir: PathBuf, reason: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.tree(&dir)?;
            lock(&self.locked).insert(dir, reason);
            Ok(())
        })
    }

    fn unlock(&self, _root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            lock(&self.locked).remove(&dir);
            Ok(())
        })
    }

    fn free_space(&self, _dir: PathBuf) -> BoxFuture<'_, Result<u64>> {
        let free = *lock(&self.free);
        Box::pin(async move {
            free.ok_or_else(|| {
                crate::protocol::ProtocolError::new(
                    crate::protocol::ErrorCode::Unavailable,
                    "no free space set",
                )
            })
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
        assert!(
            w.remove("/main".into(), "/wt".into(), Vec::new())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_pick_keeps_the_workers_changes_and_drops_what_both_sides_hold() {
        let w = MemoryWorktrees::default();
        // The person's checkout: committed `a`, uncommitted `mine`.
        w.put("/main", "a", "1\n");
        let base = w.snapshot("/main".into()).await.unwrap_or_default();
        w.put("/main", "mine", "draft\n");
        let start = w.snapshot("/main".into()).await.unwrap_or_default();
        w.put("/main", "b", "worker\n");
        w.delete("/main", "a");
        let reviewed = w.snapshot("/main".into()).await.unwrap_or_default();
        let pick = |base: &str| {
            w.pick(
                "/main".into(),
                base.into(),
                start.clone(),
                reviewed.clone(),
                "m".into(),
                None,
            )
        };
        let Ok(Picked::Commit(id)) = pick(&base).await else {
            panic!("no commit");
        };
        let files = w.commit_files(&id).unwrap_or_default();
        assert_eq!(files.keys().collect::<Vec<_>>(), ["b"]);

        // The base changed `a` too: a conflict, and nothing is committed.
        w.put("/other", "a", "2\n");
        let moved = w.snapshot("/other".into()).await.unwrap_or_default();
        let conflicts = pick(&moved).await;
        assert_eq!(conflicts.ok(), Some(Picked::Conflicts(vec!["a".into()])));
    }

    #[tokio::test]
    async fn include_copies_the_listed_files_and_a_lock_lasts_until_removal() {
        let w = MemoryWorktrees::default();
        w.put("/main", ".env", "KEY=1\n");
        w.put("/wt", "a.rs", "x\n");
        let env = IncludedFile {
            path: ".env".into(),
            bytes: 6,
            hash: "h".into(),
        };
        lock(&w.to_include).copied.push(env.clone());
        let included = w.include("/main".into(), "/wt".into()).await;
        assert_eq!(included.map(|i| i.copied).ok(), Some(vec![env.clone()]));
        assert_eq!(
            w.files("/wt").get(".env").map(String::as_str),
            Some("KEY=1\n")
        );
        let usage = w.usage("/wt".into(), Duration::from_secs(2)).await;
        assert_eq!(usage.map(|u| (u.bytes, u.complete)).ok(), Some((8, true)));

        let reason = "Ovra: t1/x".to_owned();
        assert!(w.lock("/main".into(), "/wt".into(), reason).await.is_ok());
        assert!(lock(&w.locked).contains_key(&PathBuf::from("/wt")));
        let removed = w.remove("/main".into(), "/wt".into(), vec![env]).await;
        assert!(removed.is_ok());
        assert!(lock(&w.locked).is_empty());
        assert_eq!(lock(&w.removed).len(), 1);
    }
}
