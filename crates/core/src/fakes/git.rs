//! [`MemoryGit`]: a git service that records what it was asked and answers
//! with what a test set up.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::lock;
use crate::path::RelPath;
use crate::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, Pulled,
    RepoStatus,
};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{Ancestry, BoxFuture, GitService, RemoteBranch, Result};

/// Answers `status` with [`MemoryGit::status`] (`None`: not a repository)
/// and the other reads with defaults; every call is recorded in
/// [`MemoryGit::calls`] as `"<method> <root> <args>"`.
#[derive(Debug, Default)]
pub struct MemoryGit {
    pub status: Mutex<Option<RepoStatus>>,
    pub branches: Mutex<Vec<BranchInfo>>,
    pub diff: Mutex<FileDiff>,
    /// What `pulled` answers.
    pub pulled: Mutex<Pulled>,
    pub calls: Mutex<Vec<String>>,
    /// What `record_upstream` answers; `Behind` at first.
    pub ancestry: Mutex<Ancestry>,
    /// When set, every changing call fails with `conflict` and this message.
    pub refuse: Mutex<Option<String>>,
    /// What `version` answers; `None`: git is not installed.
    pub version: Mutex<Option<String>>,
}

impl MemoryGit {
    /// A repository on `main` without changes.
    pub fn repo() -> Self {
        let git = Self::default();
        *lock(&git.status) = Some(RepoStatus {
            branch: Some("main".to_owned()),
            head: Some("c0".to_owned()),
            ..RepoStatus::default()
        });
        git
    }

    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    fn record(&self, method: &str, root: &Path, args: &str) {
        lock(&self.calls).push(format!("{method} {} {args}", root.display()));
    }

    fn change<T: Send + 'static>(
        &self,
        method: &str,
        root: &Path,
        args: &str,
        value: T,
    ) -> BoxFuture<'static, Result<T>> {
        self.record(method, root, args);
        let result = match lock(&self.refuse).clone() {
            Some(why) => Err(ProtocolError::new(ErrorCode::Conflict, why)),
            None => Ok(value),
        };
        Box::pin(async move { result })
    }

    fn read<T: Send + 'static>(
        &self,
        method: &str,
        root: &Path,
        args: &str,
        value: T,
    ) -> BoxFuture<'static, Result<T>> {
        self.record(method, root, args);
        Box::pin(async move { Ok(value) })
    }
}

impl MemoryGit {
    /// The status's upstream, split at its first slash.
    fn remote_branch(&self) -> Option<RemoteBranch> {
        let upstream = lock(&self.status)
            .as_ref()
            .and_then(|s| s.upstream.clone())?;
        let (remote, branch) = upstream.split_once('/')?;
        Some(RemoteBranch {
            remote: remote.to_owned(),
            branch: branch.to_owned(),
        })
    }

    /// A change that needs the upstream to still be `expect`.
    fn on_upstream<T: Send + 'static>(
        &self,
        method: &str,
        root: &Path,
        expect: RemoteBranch,
        commit: &str,
        value: T,
    ) -> BoxFuture<'static, Result<T>> {
        if self.remote_branch() != Some(expect) {
            self.record(method, root, commit);
            let changed = ProtocolError::new(ErrorCode::Conflict, "the branch changed");
            return Box::pin(async move { Err(changed) });
        }
        self.change(method, root, commit, value)
    }
}

fn joined(paths: &[RelPath]) -> String {
    paths
        .iter()
        .map(RelPath::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn commit(summary: &str) -> CommitInfo {
    CommitInfo {
        id: "c1".to_owned(),
        author: "Test".to_owned(),
        time: 0,
        summary: summary.to_owned(),
    }
}

impl GitService for MemoryGit {
    fn version(&self) -> BoxFuture<'_, Result<Option<String>>> {
        let version = lock(&self.version).clone();
        Box::pin(async move { Ok(version) })
    }
    fn status(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        let status = lock(&self.status).clone();
        self.read("status", &root, "", status)
    }
    /// Refused in a repository; else the repository of a fresh `git init`.
    fn init(&self, root: PathBuf) -> BoxFuture<'_, Result<()>> {
        if lock(&self.status).is_some() {
            self.record("init", &root, "");
            let refused = ProtocolError::new(ErrorCode::Conflict, "already a git repository");
            return Box::pin(async move { Err(refused) });
        }
        if lock(&self.refuse).is_none() {
            *lock(&self.status) = Some(RepoStatus {
                branch: Some("main".to_owned()),
                ..RepoStatus::default()
            });
        }
        self.change("init", &root, "", ())
    }
    fn stage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.change("stage", &root, &joined(&paths), ())
    }
    fn unstage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.change("unstage", &root, &joined(&paths), ())
    }
    fn stage_hunk(
        &self,
        root: PathBuf,
        path: RelPath,
        hunk: Hunk,
        unstage: bool,
    ) -> BoxFuture<'_, Result<()>> {
        let args = format!("{} @{} {unstage}", path.as_str(), hunk.new_start);
        self.change("stage_hunk", &root, &args, ())
    }
    fn discard(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.change("discard", &root, &joined(&paths), ())
    }
    fn commit(
        &self,
        root: PathBuf,
        message: String,
        amend: bool,
    ) -> BoxFuture<'_, Result<Committed>> {
        let committed = Committed {
            commit: commit(message.lines().next().unwrap_or_default()),
            output: String::new(),
        };
        self.change("commit", &root, &format!("{message:?} {amend}"), committed)
    }
    fn branches(&self, root: PathBuf) -> BoxFuture<'_, Result<Vec<BranchInfo>>> {
        let branches = lock(&self.branches).clone();
        self.read("branches", &root, "", branches)
    }
    fn switch(&self, root: PathBuf, branch: String) -> BoxFuture<'_, Result<()>> {
        self.change("switch", &root, &branch, ())
    }
    fn cancel(&self, root: PathBuf) -> BoxFuture<'_, Result<()>> {
        self.read("cancel", &root, "", ())
    }
    fn create_branch(
        &self,
        root: PathBuf,
        name: String,
        switch: bool,
    ) -> BoxFuture<'_, Result<()>> {
        self.change("create_branch", &root, &format!("{name} {switch}"), ())
    }
    fn diff(&self, root: PathBuf, path: RelPath, staged: bool) -> BoxFuture<'_, Result<FileDiff>> {
        let diff = FileDiff {
            path: path.as_str().to_owned(),
            ..lock(&self.diff).clone()
        };
        self.read("diff", &root, &format!("{} {staged}", path.as_str()), diff)
    }
    fn show(
        &self,
        root: PathBuf,
        path: RelPath,
        rev: GitRev,
    ) -> BoxFuture<'_, Result<GitFileText>> {
        let text = GitFileText {
            text: None,
            exists: false,
        };
        self.read("show", &root, &format!("{} {rev:?}", path.as_str()), text)
    }
    fn blame(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<Blame>> {
        self.read("blame", &root, path.as_str(), Blame::default())
    }
    fn log(&self, root: PathBuf, limit: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>> {
        self.read("log", &root, &limit.to_string(), vec![commit("first")])
    }
    fn pulled(&self, root: PathBuf) -> BoxFuture<'_, Result<Pulled>> {
        let pulled = lock(&self.pulled).clone();
        self.read("pulled", &root, "", pulled)
    }
    fn upstream(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RemoteBranch>>> {
        let found = self.remote_branch();
        self.read("upstream", &root, "", found)
    }
    fn record_upstream(
        &self,
        root: PathBuf,
        expect: RemoteBranch,
        commit: String,
    ) -> BoxFuture<'_, Result<Ancestry>> {
        let ancestry = *lock(&self.ancestry);
        self.on_upstream("record_upstream", &root, expect, &commit, ancestry)
    }
    fn fast_forward(
        &self,
        root: PathBuf,
        expect: RemoteBranch,
        commit: String,
    ) -> BoxFuture<'_, Result<()>> {
        self.on_upstream("fast_forward", &root, expect, &commit, ())
    }
    /// Sets the status's upstream to `remote/branch` when `branch` is still
    /// current and has none.
    fn set_upstream(
        &self,
        root: PathBuf,
        branch: String,
        remote: String,
        commit: String,
    ) -> BoxFuture<'_, Result<()>> {
        let mut status = lock(&self.status);
        let current = status
            .as_mut()
            .filter(|s| s.branch.as_deref() == Some(branch.as_str()) && s.upstream.is_none());
        let Some(current) = current else {
            drop(status);
            self.record("set_upstream", &root, &commit);
            let changed = ProtocolError::new(ErrorCode::Conflict, "the branch changed");
            return Box::pin(async move { Err(changed) });
        };
        current.upstream = Some(format!("{remote}/{branch}"));
        drop(status);
        self.change(
            "set_upstream",
            &root,
            &format!("{remote} {branch} {commit}"),
            (),
        )
    }
}
