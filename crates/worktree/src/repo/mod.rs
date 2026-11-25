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

use std::path::PathBuf;

use ovra_core::path::RelPath;
use ovra_core::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, RepoStatus,
};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, GitService, Result};

use crate::blocking;

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
            "this part of the git panel is not implemented yet",
        ))
    })
}

impl GitService for GitRepo {
    fn status(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        blocking(move || status::status(&root))
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
