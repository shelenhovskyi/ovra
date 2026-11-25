//! Pushes and pull requests with the person's installed `gh` and `git`
//! (`docs/specs/pr-flow.md`, ADR 0010): the only place Ovra starts
//! processes that use the person's GitHub login. It never reads, copies or
//! passes on their credentials, never calls GitHub's API itself and never
//! passes `--show-token`. Security code: changes need a separate human
//! review.
//! Contract: `ovra_core::services::Forge`; fake:
//! `ovra_core::fakes::MemoryForge`.
//!
//! A stub for now: every call answers `unavailable` until PR.A.

use std::path::PathBuf;

use ovra_core::protocol::pr::{GhStatus, PrDraft, PrInfo, PrStatus};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BoxFuture, Forge, Result};

/// The forge through the `gh` and `git` on the person's `PATH`.
#[derive(Debug, Default)]
pub struct GhForge {}

impl GhForge {
    pub fn new() -> Self {
        Self {}
    }
}

fn not_yet<T: Send + 'static>() -> BoxFuture<'static, Result<T>> {
    Box::pin(async {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "pull requests are not available in this build",
        ))
    })
}

impl Forge for GhForge {
    fn status(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<GhStatus>> {
        not_yet()
    }

    fn fetch(&self, _: PathBuf, _: String, _: String) -> BoxFuture<'_, Result<String>> {
        not_yet()
    }

    fn push(
        &self,
        _: PathBuf,
        _: String,
        _: String,
        _: String,
        _: Option<String>,
    ) -> BoxFuture<'_, Result<()>> {
        not_yet()
    }

    fn create(&self, _: PathBuf, _: PrDraft) -> BoxFuture<'_, Result<PrInfo>> {
        not_yet()
    }

    fn view(&self, _: PathBuf, _: u64) -> BoxFuture<'_, Result<PrStatus>> {
        not_yet()
    }

    fn failed_log(&self, _: PathBuf, _: u64) -> BoxFuture<'_, Result<String>> {
        not_yet()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_call_is_unavailable_until_it_is_implemented() {
        let forge = GhForge::new();
        let status = forge.status("/p".into(), "origin".into()).await;
        assert_eq!(status.err().map(|e| e.code), Some(ErrorCode::Unavailable));
    }
}
