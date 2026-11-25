//! The agent browser: the user's own Chromium per agent session, started
//! through the `Sandbox` and driven over `--remote-debugging-pipe`
//! (`docs/specs/servers-and-browser.md`, ADR 0009).
//! Contract: `ovra_core::services::Browser`.
//!
//! A stub for now: every page answers `unavailable` until task B.A.

use std::sync::Arc;

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::browser::{InputEvent, PageId, PageInfo, Pick};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    BoxFuture, Browser, BrowserSink, ConsoleEntry, ElementRef, Inspected, NetworkEntry, PageAction,
    Result, SandboxProfile, SandboxStatus,
};

mod cdp;
mod input;
mod launch;
mod snapshot;

const NOT_YET: &str = "the agent browser is not built yet";

/// The real agent browser.
#[derive(Debug, Default)]
pub struct ChromiumBrowser {}

impl ChromiumBrowser {
    pub fn new() -> Self {
        Self {}
    }
}

fn unavailable<T>() -> Result<T> {
    Err(ProtocolError::new(ErrorCode::Unavailable, NOT_YET))
}

fn later<T: Send + 'static>() -> BoxFuture<'static, Result<T>> {
    Box::pin(async { unavailable() })
}

impl Browser for ChromiumBrowser {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Unavailable {
            reason: NOT_YET.into(),
        }
    }

    fn open(
        &self,
        _: SessionId,
        _: SandboxProfile,
        _: String,
        _: Option<(u32, u32)>,
        _: Arc<dyn BrowserSink>,
    ) -> BoxFuture<'_, Result<PageInfo>> {
        later()
    }

    fn close(&self, _: &PageId) -> BoxFuture<'_, Result<()>> {
        later()
    }

    fn end(&self, _: &SessionId) {}

    fn screencast(&self, _: &PageId, _: bool) -> Result<()> {
        unavailable()
    }

    fn input(&self, _: &PageId, _: InputEvent) -> BoxFuture<'_, Result<()>> {
        later()
    }

    fn snapshot(
        &self,
        _: &PageId,
        _: Option<String>,
        _: Option<u32>,
    ) -> BoxFuture<'_, Result<String>> {
        later()
    }

    fn act(&self, _: &PageId, _: PageAction) -> BoxFuture<'_, Result<()>> {
        later()
    }

    fn console(&self, _: &PageId, _: u64, _: usize) -> Result<Vec<ConsoleEntry>> {
        unavailable()
    }

    fn network(&self, _: &PageId, _: u64, _: usize) -> Result<Vec<NetworkEntry>> {
        unavailable()
    }

    fn screenshot(&self, _: &PageId, _: Option<String>, _: bool) -> BoxFuture<'_, Result<Vec<u8>>> {
        later()
    }

    fn pick_at(&self, _: &PageId, _: i32, _: i32, _: u32) -> BoxFuture<'_, Result<Pick>> {
        later()
    }

    fn inspect(&self, _: &PageId, _: ElementRef) -> BoxFuture<'_, Result<Inspected>> {
        later()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_answers_unavailable() {
        let browser = ChromiumBrowser::new();
        assert!(matches!(
            browser.status(),
            SandboxStatus::Unavailable { .. }
        ));
        let page = PageId("pg1".into());
        let codes = [
            browser.screencast(&page, true).err(),
            browser.console(&page, 0, 10).err(),
            browser.network(&page, 0, 10).err(),
        ]
        .map(|e| e.map(|e| e.code));
        assert_eq!(codes, [Some(ErrorCode::Unavailable); 3]);
    }
}
