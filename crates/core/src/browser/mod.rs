//! The agent browser's core state (`docs/specs/servers-and-browser.md`,
//! ADR 0009): which session each page belongs to, the person's answers
//! to its navigations and the elements the person picked. Pages themselves are the [`Browser`] service's.
//!
//! A session's browser runs in a sandbox of its own that sees none of the
//! project, behind the session's netproxy, so the allowlist and the
//! loopback grants apply to it as to the agent. Nothing here calls the service with the lock held:
//! it may report through the sink at once.

mod navigation;
mod picks;
mod tools;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::watch;

use crate::Core;
use crate::core::WeakCore;
use crate::protocol::agents::{SandboxNetwork, SandboxReport, SessionId};
use crate::protocol::browser::{
    BrowserAvailability, BrowserEvent, BrowserRefusal, BrowserRequest, BrowserResponse,
    FrameMetadata, PageId, PageInfo, Pick,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{
    BoxFuture, Browser, BrowserSink, ProfileNetwork, SandboxProfile, SandboxStatus,
};

pub(crate) use navigation::session_ended;
pub(crate) use picks::{pending, sent};

type Result<T> = std::result::Result<T, ProtocolError>;

/// At most this many pages per session (the spec's limit).
pub(crate) const MAX_PAGES: usize = 4;
/// The viewport when `open` names only one side, or none.
const DEFAULT_SIZE: (u32, u32) = (1280, 800);
/// The largest viewport side, in CSS pixels.
const MAX_SIDE: u32 = 4096;

#[derive(Debug)]
pub(crate) struct BrowserState {
    browser: Arc<dyn Browser>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    sessions: BTreeMap<SessionId, Pages>,
}

/// One session's browser, for this run of the session.
#[derive(Debug, Default)]
struct Pages {
    /// Open pages, in the order they opened.
    open: Vec<PageInfo>,
    /// Every page the session had, open or closed.
    had: BTreeSet<PageId>,
    /// Pages being opened, counted against [`MAX_PAGES`].
    opening: usize,
    /// The person's answers by origin.
    answers: BTreeMap<String, bool>,
    /// Navigations waiting for the person, by origin.
    asks: BTreeMap<String, watch::Sender<Option<bool>>>,
    /// Picks for the session's next message, with their pages.
    picks: Vec<(PageId, Pick)>,
    /// Picks its messages carried, the latest last.
    sent: Vec<(PageId, Pick)>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl BrowserState {
    /// The session's browser process while it runs.
    pub(crate) fn pid(&self, session: &SessionId) -> Option<u32> {
        self.browser.pid(session)
    }

    pub(crate) fn new(browser: Arc<dyn Browser>) -> Self {
        Self {
            browser,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

pub(crate) async fn handle(core: &Core, request: BrowserRequest) -> Result<Response> {
    let browser = core.browser_state().browser.clone();
    // `status` says why, even when no browser runs here.
    if !matches!(request, BrowserRequest::Status { .. }) {
        available(browser.as_ref())?;
    }
    let answer = match request {
        BrowserRequest::Status { session } => {
            BrowserResponse::Status(availability(core, &session)?)
        }
        BrowserRequest::Open {
            session,
            url,
            width,
            height,
        } => {
            let size = match (width, height) {
                (None, None) => None,
                (w, h) => Some((w.unwrap_or(DEFAULT_SIZE.0), h.unwrap_or(DEFAULT_SIZE.1))),
            };
            BrowserResponse::Page(open(core, &session, url, size).await?)
        }
        BrowserRequest::Close { page } => {
            owner(core, &page)?;
            browser.close(&page).await?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        BrowserRequest::Screencast { page, on } => {
            owner(core, &page)?;
            browser.screencast(&page, on)?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        BrowserRequest::Input { page, event } => {
            owner(core, &page)?;
            browser.input(&page, event).await?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        BrowserRequest::Pick { page, x, y, up } => {
            owner(core, &page)?;
            BrowserResponse::Picked(Box::new(browser.pick_at(&page, x, y, up).await?))
        }
        BrowserRequest::Attach { page, picks } => {
            picks::attach(core, &page, picks)?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        BrowserRequest::AnswerNavigation {
            page,
            origin,
            allow,
        } => {
            navigation::answer(core, &page, origin, allow)?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
    };
    Ok(Response::Browser(answer))
}

/// Opens `url` in a new page of `session`'s browser, starting it in the
/// session's sandbox profile if it does not run.
pub(crate) async fn open(
    core: &Core,
    session: &SessionId,
    url: String,
    size: Option<(u32, u32)>,
) -> Result<PageInfo> {
    let state = core.browser_state();
    available(state.browser.as_ref())?;
    if url != "about:blank" {
        crate::core::check_web_url(&url)?;
    }
    if size.is_some_and(|(w, h)| !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h)) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("width and height are 1 to {MAX_SIDE}"),
        ));
    }
    let profile = profile(core, session)?;
    {
        let mut st = state.lock();
        let pages = st.sessions.entry(session.clone()).or_default();
        if pages.open.len() + pages.opening >= MAX_PAGES {
            return Err(conflict(format!(
                "a session has {MAX_PAGES} pages at most: close one first"
            )));
        }
        pages.opening += 1;
    }
    let sink = Arc::new(Sink {
        core: core.downgrade(),
        session: session.clone(),
    });
    let opened = (state.browser)
        .open(session.clone(), profile, url, size, sink)
        .await;
    let mut st = state.lock();
    let Some(pages) = st.sessions.get_mut(session) else {
        // The session ended meanwhile, and its browser with it.
        drop(st);
        state.browser.end(session);
        return Err(conflict("the session has ended"));
    };
    pages.opening -= 1;
    let info = opened?;
    upsert(core, session, pages, info.clone());
    Ok(info)
}

/// What the session's browser may touch: its own fresh profile folder,
/// which the service makes and puts in `work_dir` (the core passes an
/// empty placeholder, so a service that does not replace it cannot start
/// one), and the session's netproxy as its only way out. Never the
/// project: Chromium runs with `--no-sandbox` inside Ovra's sandbox, so a
/// page that breaks its renderer gets what this profile allows. Only for
/// a session whose agent runs in the sandbox behind that netproxy.
fn profile(core: &Core, session: &SessionId) -> Result<SandboxProfile> {
    let (info, network) = core.sessions().find(session, |s| (s.info(), s.network()))?;
    if info.status.is_ended() {
        return Err(conflict("the session has ended"));
    }
    let proxied = matches!(
        info.sandbox,
        Some(SandboxReport {
            network: SandboxNetwork::Proxy,
            ..
        })
    );
    match network {
        Some(network @ ProfileNetwork::Proxy { .. }) if proxied => Ok(SandboxProfile {
            more_work_dirs: Vec::new(),
            kind: crate::services::ProfileKind::Browser,
            agent_state: None,
            project: None,
            work_env: None,
            work_dir: PathBuf::new(),
            work_dir_read_only: false,
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: Vec::new(),
            network,
        }),
        _ => Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "the agent browser runs only for sessions whose agent runs in the sandbox behind \
             its own netproxy, and this session's agent does not",
        )),
    }
}

/// Whether `session`'s browser can run, and if not, why, most basic first:
/// the session ended, no browser here, or not an agent in the sandbox.
fn availability(core: &Core, session: &SessionId) -> Result<BrowserAvailability> {
    let refused = |code, reason| Ok(BrowserAvailability::Refused { code, reason });
    let profile = profile(core, session);
    match &profile {
        Err(e) if e.code == ErrorCode::Conflict => {
            return refused(BrowserRefusal::SessionEnded, e.message.clone());
        }
        Err(e) if e.code != ErrorCode::Unavailable => return Err(e.clone()),
        _ => {}
    }
    let browser = core.browser_state().browser.as_ref();
    if let SandboxStatus::Unavailable { reason } = browser.status() {
        // A service that gives no code is not here, for this build.
        let code = browser.refusal().unwrap_or(BrowserRefusal::NotSupported);
        return refused(code, reason);
    }
    match profile {
        Ok(_) => Ok(BrowserAvailability::Available),
        Err(e) => refused(BrowserRefusal::NotSandboxed, e.message),
    }
}

/// The session an open page belongs to.
pub(crate) fn owner(core: &Core, page: &PageId) -> Result<SessionId> {
    let st = core.browser_state().lock();
    st.sessions
        .iter()
        .find(|(_, p)| p.open.iter().any(|i| i.id == *page))
        .map(|(s, _)| s.clone())
        .ok_or_else(|| page_not_found(page))
}

/// The session's open pages.
pub(crate) fn pages(core: &Core, session: &SessionId) -> Vec<PageInfo> {
    let st = core.browser_state().lock();
    st.sessions
        .get(session)
        .map(|p| p.open.clone())
        .unwrap_or_default()
}

/// Adds or updates a page and tells every front end.
fn upsert(core: &Core, session: &SessionId, pages: &mut Pages, info: PageInfo) {
    if info.session != *session {
        return;
    }
    pages.had.insert(info.id.clone());
    match pages.open.iter_mut().find(|p| p.id == info.id) {
        Some(known) if *known == info => return,
        Some(known) => *known = info,
        None => pages.open.push(info),
    }
    emit_pages(core, session, pages);
}

fn emit_pages(core: &Core, session: &SessionId, pages: &Pages) {
    core.emit(BrowserEvent::Pages {
        session: session.clone(),
        pages: pages.open.clone(),
    });
}

/// How one session's browser reports; holds the core weakly.
#[derive(Debug)]
struct Sink {
    core: WeakCore,
    session: SessionId,
}

impl BrowserSink for Sink {
    fn frame(&self, page: &PageId, jpeg: String, metadata: FrameMetadata) {
        let Some(core) = self.core.upgrade() else {
            return;
        };
        let open = pages(&core, &self.session).iter().any(|p| p.id == *page);
        if open {
            core.emit(BrowserEvent::Frame {
                page: page.clone(),
                jpeg,
                metadata,
            });
        }
    }

    fn page(&self, info: PageInfo) {
        if let Some(core) = self.core.upgrade() {
            let mut st = core.browser_state().lock();
            // Pages of an ended session are gone with it.
            if let Some(pages) = st.sessions.get_mut(&self.session) {
                upsert(&core, &self.session, pages, info);
            }
        }
    }

    fn closed(&self, page: &PageId) {
        if let Some(core) = self.core.upgrade() {
            let mut st = core.browser_state().lock();
            if let Some(pages) = st.sessions.get_mut(&self.session) {
                let before = pages.open.len();
                pages.open.retain(|p| p.id != *page);
                if pages.open.len() != before {
                    emit_pages(&core, &self.session, pages);
                }
            }
        }
    }

    fn navigation(&self, page: &PageId, origin: String) -> BoxFuture<'_, bool> {
        let page = page.clone();
        Box::pin(async move {
            match self.core.upgrade() {
                Some(core) => navigation::decide(&core, &self.session, &page, origin).await,
                None => false,
            }
        })
    }
}

/// `scheme://host[:port]` of a URL, as the browser names origins.
pub(crate) fn origin(url: &str) -> String {
    let start = url.find("://").map_or(0, |i| i + 3);
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    url[..end].to_owned()
}

fn available(browser: &dyn Browser) -> Result<()> {
    match browser.status() {
        SandboxStatus::Unavailable { reason } => {
            Err(ProtocolError::new(ErrorCode::Unavailable, reason))
        }
        SandboxStatus::Available { .. } => Ok(()),
    }
}

fn page_not_found(page: &PageId) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("page {} not found", page.0))
}

fn conflict(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}

#[cfg(test)]
mod tests;
