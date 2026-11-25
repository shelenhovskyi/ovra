//! [`FakeBrowser`]: pages as plain records; no browser runs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::{lock, not_found};
use crate::protocol::agents::SessionId;
use crate::protocol::browser::{FrameMetadata, InputEvent, PageId, PageInfo, Pick};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    BoxFuture, Browser, BrowserSink, ConsoleEntry, ElementRef, Inspected, NetworkEntry, PageAction,
    Result, SandboxProfile, SandboxStatus,
};

/// The PNG every screenshot and inspection answers.
pub const FAKE_PNG: &[u8] = b"fake png";

/// Pages `pg1`, `pg2`, ... Opening a page and `PageAction::Navigate` ask
/// the sink about the target's origin and fail with `forbidden` when it
/// says no; every other action and input is only recorded. Snapshots,
/// console and network answer what tests put in the page; `pick_at` and
/// `inspect` answer [`FakeBrowser::element`].
#[derive(Debug, Default)]
pub struct FakeBrowser {
    /// When set, `open` fails with `unavailable`.
    pub fails: bool,
    pub pages: Mutex<BTreeMap<PageId, FakePage>>,
    /// The profile of each browser started, in order.
    pub started: Mutex<Vec<(SessionId, SandboxProfile)>>,
    /// The element under every point; `None` answers `not_found`.
    pub element: Mutex<Option<Pick>>,
    /// A frame (JPEG, base64) sent whenever a page's screencast turns on.
    pub on_screencast: Mutex<Option<String>>,
    sinks: Mutex<BTreeMap<SessionId, Arc<dyn BrowserSink>>>,
    count: Mutex<u64>,
}

#[derive(Debug, Clone)]
pub struct FakePage {
    pub info: PageInfo,
    pub screencast: bool,
    /// What `snapshot` answers.
    pub snapshot: String,
    pub console: Vec<ConsoleEntry>,
    pub network: Vec<NetworkEntry>,
    pub actions: Vec<PageAction>,
    pub inputs: Vec<InputEvent>,
}

/// `scheme://host[:port]` of a URL.
fn origin(url: &str) -> String {
    let start = url.find("://").map_or(0, |i| i + 3);
    let end = url[start..].find('/').map_or(url.len(), |i| start + i);
    url[..end].to_owned()
}

fn refused(origin: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Forbidden, format!("{origin} was not allowed"))
}

impl FakeBrowser {
    /// Changes a page as a test needs; `false` if there is no such page.
    pub fn with_page(&self, page: &PageId, change: impl FnOnce(&mut FakePage)) -> bool {
        lock(&self.pages).get_mut(page).map(change).is_some()
    }

    /// A copy of the page as it is now.
    pub fn page(&self, page: &PageId) -> Option<FakePage> {
        lock(&self.pages).get(page).cloned()
    }

    /// Sends a frame through the session's sink if the screencast is on.
    pub fn frame(&self, page: &PageId, jpeg: &str) -> bool {
        let Some((session, true)) = self
            .page(page)
            .map(|p| (p.info.session.clone(), p.screencast))
        else {
            return false;
        };
        let Some(sink) = lock(&self.sinks).get(&session).cloned() else {
            return false;
        };
        let metadata = FrameMetadata {
            width: 1280,
            height: 800,
            scroll_x: 0,
            scroll_y: 0,
        };
        sink.frame(page, jpeg.to_owned(), metadata);
        true
    }

    fn sink_of(&self, page: &PageId) -> Result<Arc<dyn BrowserSink>> {
        let session = self
            .page(page)
            .ok_or_else(|| not_found(format!("page {}", page.0)))?
            .info
            .session;
        lock(&self.sinks)
            .get(&session)
            .cloned()
            .ok_or_else(|| not_found(format!("the browser of {}", session.0)))
    }

    fn change(&self, page: &PageId, change: impl FnOnce(&mut FakePage)) -> Result<()> {
        self.with_page(page, change)
            .then_some(())
            .ok_or_else(|| not_found(format!("page {}", page.0)))
    }

    fn pick(&self, page: &PageId) -> Result<Pick> {
        let url = self
            .page(page)
            .ok_or_else(|| not_found(format!("page {}", page.0)))?
            .info
            .url;
        let mut pick = lock(&self.element)
            .clone()
            .ok_or_else(|| not_found("an element there"))?;
        let mut count = lock(&self.count);
        *count += 1;
        pick.pick_id = format!("pk{count}");
        pick.url = url;
        Ok(pick)
    }
}

impl Browser for FakeBrowser {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Available {
            name: "fake".into(),
            gaps: Vec::new(),
        }
    }

    fn open(
        &self,
        session: SessionId,
        profile: SandboxProfile,
        url: String,
        _: Option<(u32, u32)>,
        sink: Arc<dyn BrowserSink>,
    ) -> BoxFuture<'_, Result<PageInfo>> {
        Box::pin(async move {
            if self.fails {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "the fake browser fails",
                ));
            }
            let sink = {
                let mut sinks = lock(&self.sinks);
                if !sinks.contains_key(&session) {
                    lock(&self.started).push((session.clone(), profile));
                }
                sinks.entry(session.clone()).or_insert(sink).clone()
            };
            let id = {
                let mut count = lock(&self.count);
                *count += 1;
                PageId(format!("pg{count}"))
            };
            let target = origin(&url);
            if !sink.navigation(&id, target.clone()).await {
                return Err(refused(&target));
            }
            let info = PageInfo {
                id: id.clone(),
                session,
                url,
                title: String::new(),
            };
            let page = FakePage {
                info: info.clone(),
                screencast: false,
                snapshot: String::new(),
                console: Vec::new(),
                network: Vec::new(),
                actions: Vec::new(),
                inputs: Vec::new(),
            };
            lock(&self.pages).insert(id, page);
            sink.page(info.clone());
            Ok(info)
        })
    }

    fn close(&self, page: &PageId) -> BoxFuture<'_, Result<()>> {
        let page = page.clone();
        Box::pin(async move {
            let sink = self.sink_of(&page)?;
            lock(&self.pages).remove(&page);
            sink.closed(&page);
            Ok(())
        })
    }

    fn end(&self, session: &SessionId) {
        let Some(sink) = lock(&self.sinks).remove(session) else {
            return;
        };
        // Reported after the lock is gone: a sink may look at the pages.
        let ended: Vec<PageId> = {
            let mut pages = lock(&self.pages);
            let ended = pages
                .values()
                .filter(|p| p.info.session == *session)
                .map(|p| p.info.id.clone())
                .collect();
            pages.retain(|_, p| p.info.session != *session);
            ended
        };
        for page in ended {
            sink.closed(&page);
        }
    }

    fn screencast(&self, page: &PageId, on: bool) -> Result<()> {
        self.change(page, |p| p.screencast = on)?;
        let frame = lock(&self.on_screencast).clone();
        if let (true, Some(jpeg)) = (on, frame) {
            self.frame(page, &jpeg);
        }
        Ok(())
    }

    fn input(&self, page: &PageId, event: InputEvent) -> BoxFuture<'_, Result<()>> {
        let result = self.change(page, |p| p.inputs.push(event));
        Box::pin(async move { result })
    }

    fn snapshot(
        &self,
        page: &PageId,
        _: Option<String>,
        _: Option<u32>,
    ) -> BoxFuture<'_, Result<String>> {
        let result = self
            .page(page)
            .map(|p| p.snapshot)
            .ok_or_else(|| not_found(format!("page {}", page.0)));
        Box::pin(async move { result })
    }

    fn act(&self, page: &PageId, action: PageAction) -> BoxFuture<'_, Result<()>> {
        let page = page.clone();
        Box::pin(async move {
            let sink = self.sink_of(&page)?;
            if let PageAction::Navigate { url } = &action {
                let target = origin(url);
                if !sink.navigation(&page, target.clone()).await {
                    return Err(refused(&target));
                }
                let mut info = None;
                self.change(&page, |p| {
                    p.info.url.clone_from(url);
                    info = Some(p.info.clone());
                })?;
                sink.page(info.ok_or_else(|| not_found(format!("page {}", page.0)))?);
            }
            self.change(&page, |p| p.actions.push(action))
        })
    }

    fn console(&self, page: &PageId, since: u64, limit: usize) -> Result<Vec<ConsoleEntry>> {
        let page = self
            .page(page)
            .ok_or_else(|| not_found(format!("page {}", page.0)))?;
        Ok(page
            .console
            .into_iter()
            .filter(|e| e.n > since)
            .take(limit)
            .collect())
    }

    fn network(&self, page: &PageId, since: u64, limit: usize) -> Result<Vec<NetworkEntry>> {
        let page = self
            .page(page)
            .ok_or_else(|| not_found(format!("page {}", page.0)))?;
        Ok(page
            .network
            .into_iter()
            .filter(|e| e.n > since)
            .take(limit)
            .collect())
    }

    fn screenshot(
        &self,
        page: &PageId,
        _: Option<String>,
        _: bool,
    ) -> BoxFuture<'_, Result<Vec<u8>>> {
        let result = self
            .page(page)
            .map(|_| FAKE_PNG.to_vec())
            .ok_or_else(|| not_found(format!("page {}", page.0)));
        Box::pin(async move { result })
    }

    fn pick_at(&self, page: &PageId, _: i32, _: i32, _: u32) -> BoxFuture<'_, Result<Pick>> {
        let result = self.pick(page);
        Box::pin(async move { result })
    }

    fn inspect(&self, page: &PageId, _: ElementRef) -> BoxFuture<'_, Result<Inspected>> {
        let result = self.pick(page).map(|element| Inspected {
            element,
            png: FAKE_PNG.to_vec(),
        });
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ProfileNetwork;

    /// Allows `http://localhost:5173` only; records what it is told.
    #[derive(Debug, Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl BrowserSink for Recorder {
        fn frame(&self, page: &PageId, jpeg: String, _: FrameMetadata) {
            lock(&self.0).push(format!("frame {} {jpeg}", page.0));
        }
        fn page(&self, info: PageInfo) {
            lock(&self.0).push(format!("page {} {}", info.id.0, info.url));
        }
        fn closed(&self, page: &PageId) {
            lock(&self.0).push(format!("closed {}", page.0));
        }
        fn navigation(&self, _: &PageId, origin: String) -> BoxFuture<'_, bool> {
            lock(&self.0).push(format!("asked {origin}"));
            Box::pin(async move { origin == "http://localhost:5173" })
        }
    }

    fn profile() -> SandboxProfile {
        SandboxProfile {
            work_dir: "/w".into(),
            read: Vec::new(),
            write: Vec::new(),
            expose: Vec::new(),
            forward: vec![5173],
            network: ProfileNetwork::Blocked,
        }
    }

    #[tokio::test]
    async fn navigation_asks_the_sink_and_end_closes_every_page() {
        let browser = FakeBrowser::default();
        let sink = Arc::new(Recorder::default());
        let s1 = SessionId("s1".into());
        let page = browser
            .open(
                s1.clone(),
                profile(),
                "http://localhost:5173/a".into(),
                None,
                sink.clone(),
            )
            .await
            .expect("open")
            .id;
        let away = PageAction::Navigate {
            url: "https://example.com/x".into(),
        };
        let refused = browser.act(&page, away).await;
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::Forbidden));
        *lock(&browser.on_screencast) = Some("/9j/".into());
        browser.screencast(&page, true).expect("screencast");
        browser.end(&s1);
        assert!(browser.page(&page).is_none());
        assert_eq!(lock(&browser.started).len(), 1);
        assert_eq!(
            *lock(&sink.0),
            [
                "asked http://localhost:5173",
                "page pg1 http://localhost:5173/a",
                "asked https://example.com",
                "frame pg1 /9j/",
                "closed pg1",
            ]
        );
    }
}
