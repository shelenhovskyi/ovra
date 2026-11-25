//! [`MemoryGit`]: a git service that records what it was asked and answers
//! with what a test set up.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::lock;
use crate::path::RelPath;
use crate::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, RepoStatus,
};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{BoxFuture, GitService, Result};

/// Answers `status` with [`MemoryGit::status`] (`None`: not a repository)
/// and the other reads with defaults; every call is recorded in
/// [`MemoryGit::calls`] as `"<method> <root> <args>"`.
#[derive(Debug, Default)]
pub struct MemoryGit {
    pub status: Mutex<Option<RepoStatus>>,
    pub branches: Mutex<Vec<BranchInfo>>,
    pub diff: Mutex<FileDiff>,
    pub calls: Mutex<Vec<String>>,
    /// When set, every changing call fails with `conflict` and this message.
    pub refuse: Mutex<Option<String>>,
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
}
