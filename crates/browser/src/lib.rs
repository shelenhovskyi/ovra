//! The agent browser: the user's own Chromium per agent session, started
//! through the `Sandbox` and driven over `--remote-debugging-pipe`
//! (`docs/specs/servers-and-browser.md`, ADR 0009).
//! Contract: `ovra_core::services::Browser`.
//!
//! The CDP client is Ovra's own ([`cdp`]): a few hundred lines over the
//! pipe with tokio and serde_json, where `chromiumoxide` speaks CDP only
//! over a WebSocket, which would need a debugging port.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::browser::{BrowserRefusal, InputEvent, PageId, PageInfo, Pick};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{
    BoxFuture, Browser, BrowserSink, ConsoleEntry, ElementRef, Inspected, NetworkEntry, PageAction,
    Result, Sandbox, SandboxProfile, SandboxStatus,
};

mod cdp;
mod input;
mod launch;
mod logs;
mod origin;
mod page;
mod pick;
mod session;
mod snapshot;

use launch::Launch;
use page::Page;
use session::{Session, Sessions, lock};

/// At most this many pages per session (the spec's limit).
const PAGES_PER_SESSION: usize = 4;

/// The viewport when `open` names none.
const DEFAULT_SIZE: (u32, u32) = (1280, 800);

/// The real agent browser.
#[derive(Debug)]
pub struct ChromiumBrowser {
    launcher: Option<Box<dyn Launch>>,
    status: SandboxStatus,
    /// Why `status` is unavailable; unused while it is available.
    refusal: BrowserRefusal,
    sessions: Sessions,
    /// One browser starts at a time, so a session never gets two.
    starting: tokio::sync::Mutex<()>,
    /// Numbers pages and picks.
    count: AtomicU64,
}

impl ChromiumBrowser {
    /// The user's Chromium at `chromium`, else the first of `chromium`,
    /// `chromium-browser`, `google-chrome` and `google-chrome-stable` on
    /// `PATH`, run in `sandbox`. Never downloads one.
    pub fn new(sandbox: Arc<dyn Sandbox>, chromium: Option<PathBuf>) -> Self {
        let path = ovra_core::child_path::or_inherited(std::env::var_os("PATH"));
        let found = launch::find(chromium.as_deref(), path.as_deref());
        let unavailable = |code, reason: String| (SandboxStatus::Unavailable { reason }, code);
        let (status, refusal) = match (&found, sandbox.status()) {
            _ if !cfg!(unix) => unavailable(
                BrowserRefusal::NotSupported,
                "the agent browser needs Linux for now".into(),
            ),
            (_, SandboxStatus::Unavailable { reason }) => unavailable(
                BrowserRefusal::NoSandbox,
                format!(
                    "the agent browser runs only in the sandbox, which is unavailable: {reason}"
                ),
            ),
            (None, _) => unavailable(
                BrowserRefusal::NoChromium,
                match &chromium {
                    Some(path) => format!("no browser at {}", path.display()),
                    None => format!(
                        "install Chromium or Google Chrome: none of {} is on PATH",
                        launch::NAMES.join(", ")
                    ),
                },
            ),
            // What the sandbox does not cover, the browser in it does not either.
            (Some(program), SandboxStatus::Available { gaps, .. }) => (
                SandboxStatus::Available {
                    name: format!("Chromium at {}", program.display()),
                    gaps,
                },
                BrowserRefusal::NoChromium,
            ),
        };
        let launcher = found
            .filter(|_| matches!(status, SandboxStatus::Available { .. }))
            .map(|program| Box::new(launch::Chromium { program, sandbox }) as Box<dyn Launch>);
        Self {
            refusal,
            ..Self::with(launcher, status)
        }
    }

    fn with(launcher: Option<Box<dyn Launch>>, status: SandboxStatus) -> Self {
        Self {
            launcher,
            status,
            refusal: BrowserRefusal::NoChromium,
            sessions: Sessions::default(),
            starting: tokio::sync::Mutex::new(()),
            count: AtomicU64::new(0),
        }
    }

    fn next(&self) -> u64 {
        self.count.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn page(&self, id: &PageId) -> Result<Arc<Page>> {
        let sessions: Vec<Arc<Session>> = lock(&self.sessions).values().cloned().collect();
        sessions.iter().find_map(|s| s.page(id)).ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, format!("page {} not found", id.0))
        })
    }

    /// The session's browser, started in `profile` if it does not run.
    async fn session(
        &self,
        id: &SessionId,
        mut profile: SandboxProfile,
        sink: Arc<dyn BrowserSink>,
    ) -> Result<Arc<Session>> {
        let _one = self.starting.lock().await;
        if let Some(session) = lock(&self.sessions).get(id).cloned() {
            return Ok(session);
        }
        let launcher = self.launcher.as_ref().ok_or_else(|| {
            let reason = match &self.status {
                SandboxStatus::Unavailable { reason } => reason.clone(),
                SandboxStatus::Available { .. } => "the agent browser is unavailable".into(),
            };
            ProtocolError::new(ErrorCode::Unavailable, reason)
        })?;
        let dir = profile_dir(&std::env::temp_dir())?;
        // The core passes a placeholder: the browser's folder is its work dir.
        profile.work_dir.clone_from(&dir);
        // Whatever the caller said: a browser gets none of the person's state.
        profile.kind = ovra_core::services::ProfileKind::Browser;
        let launched = match launcher.launch(&profile, &dir) {
            Ok(launched) => launched,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        let session = Session::start(id.clone(), launched, sink, Some(dir), &self.sessions).await?;
        lock(&self.sessions).insert(id.clone(), session.clone());
        Ok(session)
    }
}

/// A new, empty folder in `parent` only this user can enter, with a name
/// nobody can guess ahead: made here, never one planted there.
fn profile_dir(parent: &std::path::Path) -> Result<PathBuf> {
    let failed = |e: std::io::Error| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("no profile folder for the browser: {e}"),
        )
    };
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    loop {
        // Windows has no /dev/urandom.
        let dir = parent.join(format!("ovra-browser-{:032x}", rand::random::<u128>()));
        // `create` fails on anything already there, a link included.
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(failed(e)),
        }
    }
}

impl Browser for ChromiumBrowser {
    fn status(&self) -> SandboxStatus {
        self.status.clone()
    }

    fn refusal(&self) -> BrowserRefusal {
        self.refusal
    }

    fn open(
        &self,
        session: SessionId,
        profile: SandboxProfile,
        url: String,
        size: Option<(u32, u32)>,
        sink: Arc<dyn BrowserSink>,
    ) -> BoxFuture<'_, Result<PageInfo>> {
        Box::pin(async move {
            let browser = self.session(&session, profile, sink).await?;
            if browser.pages().len() >= PAGES_PER_SESSION {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    format!("a session has {PAGES_PER_SESSION} pages at most: close one first"),
                ));
            }
            let id = PageId(format!("pg{}", self.next()));
            browser.open(id, &url, size.unwrap_or(DEFAULT_SIZE)).await
        })
    }

    fn close(&self, page: &PageId) -> BoxFuture<'_, Result<()>> {
        let page = page.clone();
        Box::pin(async move {
            let sessions: Vec<Arc<Session>> = lock(&self.sessions).values().cloned().collect();
            sessions
                .iter()
                .any(|s| s.close(&page))
                .then_some(())
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, format!("page {} not found", page.0))
                })
        })
    }

    fn end(&self, session: &SessionId) {
        let ended = lock(&self.sessions).remove(session);
        if let Some(ended) = ended {
            ended.end();
        }
    }

    fn screencast(&self, page: &PageId, on: bool) -> Result<()> {
        self.page(page)?.screencast(on);
        Ok(())
    }

    fn input(&self, page: &PageId, event: InputEvent) -> BoxFuture<'_, Result<()>> {
        let page = self.page(page);
        Box::pin(async move { page?.input(&[event]).await })
    }

    fn snapshot(
        &self,
        page: &PageId,
        target: Option<String>,
        depth: Option<u32>,
    ) -> BoxFuture<'_, Result<String>> {
        let page = self.page(page);
        Box::pin(async move { page?.snapshot(target, depth).await })
    }

    fn act(&self, page: &PageId, action: PageAction) -> BoxFuture<'_, Result<()>> {
        let page = self.page(page);
        Box::pin(async move { page?.act(action).await })
    }

    fn console(&self, page: &PageId, since: u64, limit: usize) -> Result<Vec<ConsoleEntry>> {
        Ok(self.page(page)?.console(since, limit))
    }

    fn network(&self, page: &PageId, since: u64, limit: usize) -> Result<Vec<NetworkEntry>> {
        Ok(self.page(page)?.network(since, limit))
    }

    fn screenshot(
        &self,
        page: &PageId,
        target: Option<String>,
        full_page: bool,
    ) -> BoxFuture<'_, Result<Vec<u8>>> {
        let page = self.page(page);
        Box::pin(async move { page?.screenshot(target, full_page).await })
    }

    fn pick_at(&self, page: &PageId, x: i32, y: i32, up: u32) -> BoxFuture<'_, Result<Pick>> {
        let page = self.page(page);
        Box::pin(async move { page?.pick_at(x, y, up, format!("pk{}", self.next())).await })
    }

    fn inspect(&self, page: &PageId, element: ElementRef) -> BoxFuture<'_, Result<Inspected>> {
        let page = self.page(page);
        Box::pin(async move {
            let (element, png) = page?.inspect(element, format!("pk{}", self.next())).await?;
            Ok(Inspected { element, png })
        })
    }
}

#[cfg(test)]
mod tests;
