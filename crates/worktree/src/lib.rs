//! Git worktrees for isolated sessions, working-tree snapshots for direct
//! mode (ADR 0005) and the diffs the review inbox shows.
//! Contract: `ovra_core::services::Worktrees`; fake:
//! `ovra_core::fakes::MemoryWorktrees`.
//!
//! Stub: every call answers `unavailable` until tasks 3.A and 3.F land.

use std::path::PathBuf;

use ovra_core::path::RelPath;
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Delta, Result, WorktreeInfo, Worktrees};

/// Worktrees through the `git` command line.
#[derive(Debug, Default)]
pub struct GitWorktrees {}

impl GitWorktrees {
    pub fn new() -> Self {
        Self {}
    }
}

fn not_yet<'a, T: Send + 'a>() -> BoxFuture<'a, Result<T>> {
    Box::pin(async {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "worktrees are not implemented yet (tasks 3.A, 3.F)",
        ))
    })
}

impl Worktrees for GitWorktrees {
    fn snapshot(&self, _: PathBuf) -> BoxFuture<'_, Result<String>> {
        not_yet()
    }
    fn create(
        &self,
        _: PathBuf,
        _: PathBuf,
        _: String,
        _: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>> {
        not_yet()
    }
    fn changes(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        not_yet()
    }
    fn patch(&self, _: PathBuf, _: String, _: RelPath) -> BoxFuture<'_, Result<String>> {
        not_yet()
    }
    fn restore(&self, _: PathBuf, _: String, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        not_yet()
    }
    fn apply(
        &self,
        _: PathBuf,
        _: String,
        _: PathBuf,
        _: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>> {
        not_yet()
    }
    fn commit(&self, _: PathBuf, _: Vec<RelPath>, _: String) -> BoxFuture<'_, Result<String>> {
        not_yet()
    }
    fn remove(&self, _: PathBuf, _: PathBuf) -> BoxFuture<'_, Result<()>> {
        not_yet()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn answers_unavailable() {
        let code = GitWorktrees::new()
            .snapshot(PathBuf::from("/w"))
            .await
            .err()
            .map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::Unavailable));
    }
}
