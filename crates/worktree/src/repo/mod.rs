//! The git panel's operations on a project's main checkout
//! (`docs/specs/git-panel.md`). Contract: `ovra_core::services::GitService`;
//! fake: `ovra_core::fakes::MemoryGit`.

use std::path::PathBuf;

use ovra_core::path::RelPath;
use ovra_core::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, RepoStatus,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, GitService, Result};

/// The git panel through the `git` command line.
#[derive(Debug, Default)]
pub struct GitRepo;

impl GitRepo {
    pub fn new() -> Self {
        Self
    }
}

fn later<T: Send + 'static>() -> BoxFuture<'static, Result<T>> {
    Box::pin(async {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "the git panel is not implemented yet",
        ))
    })
}

impl GitService for GitRepo {
    fn status(&self, _: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        later()
    }
    fn stage(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn unstage(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn stage_hunk(&self, _: PathBuf, _: RelPath, _: Hunk, _: bool) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn discard(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        later()
    }
    fn commit(&self, _: PathBuf, _: String, _: bool) -> BoxFuture<'_, Result<Committed>> {
        later()
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
    fn show(&self, _: PathBuf, _: RelPath, _: GitRev) -> BoxFuture<'_, Result<GitFileText>> {
        later()
    }
    fn blame(&self, _: PathBuf, _: RelPath) -> BoxFuture<'_, Result<Blame>> {
        later()
    }
    fn log(&self, _: PathBuf, _: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>> {
        later()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_stub_says_it_is_not_there_yet() {
        let status = GitRepo::new().status(PathBuf::from("/")).await;
        assert_eq!(status.map_err(|e| e.code), Err(ErrorCode::Unavailable));
    }
}
