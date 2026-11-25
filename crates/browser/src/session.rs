//! One agent session's running browser: its pipe, its process, its pages,
//! and the task that hands its events to them.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::browser::{PageId, PageInfo};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BrowserSink, Result};
use serde_json::{Value, json};
use tokio::process::Child;
use tokio::sync::{mpsc, watch};

use crate::cdp::{Cdp, Event};
use crate::launch::Launched;
use crate::page::Page;

pub(crate) type Sessions = Arc<Mutex<HashMap<SessionId, Arc<Session>>>>;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
pub(crate) struct Session {
    pub(crate) id: SessionId,
    cdp: Cdp,
    sink: Arc<dyn BrowserSink>,
    pages: Mutex<Vec<Arc<Page>>>,
    /// The targets `open` made: the only pages allowed to run.
    own: Arc<Mutex<HashSet<String>>>,
    /// How many `Target.createTarget` calls still wait for their target.
    creating: watch::Sender<usize>,
    child: Mutex<Option<Child>>,
    /// The fresh profile, removed once the browser is gone.
    profile_dir: Mutex<Option<PathBuf>>,
}

impl Session {
    /// Starts talking to a launched browser; it leaves `sessions` by
    /// itself when it closes.
    pub(crate) async fn start(
        id: SessionId,
        launched: Launched,
        sink: Arc<dyn BrowserSink>,
        profile_dir: Option<PathBuf>,
        sessions: &Sessions,
    ) -> Result<Arc<Self>> {
        let (cdp, events) = Cdp::start(launched.reader, launched.writer);
        let stderr = launched.stderr;
        let session = Arc::new(Self {
            id,
            cdp,
            sink,
            pages: Mutex::default(),
            own: Arc::default(),
            creating: watch::channel(0).0,
            child: Mutex::new(launched.child),
            profile_dir: Mutex::new(profile_dir),
        });
        tokio::spawn(dispatch(Arc::downgrade(&session), events, sessions.clone()));
        // Every new page waits for the debugger, so one the page opens
        // itself is closed before it runs (`Session::attached`).
        let attach = json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true});
        let mut ready = Ok(Value::Null);
        for (method, params) in [
            ("Target.setDiscoverTargets", json!({ "discover": true })),
            ("Target.setAutoAttach", attach),
        ] {
            ready = session.cdp.call(None, method, params).await;
            if ready.is_err() {
                break;
            }
        }
        if let Err(e) = ready {
            session.kill();
            let said = match stderr {
                Some(lines) => tokio::time::timeout(Duration::from_secs(1), lines)
                    .await
                    .ok()
                    .and_then(|joined| joined.ok())
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            let why = said
                .last()
                .map_or(e.message, |line| format!("it said: {line}"));
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("the browser did not start; {why}"),
            ));
        }
        Ok(session)
    }

    pub(crate) fn pages(&self) -> Vec<Arc<Page>> {
        lock(&self.pages).clone()
    }

    pub(crate) fn page(&self, id: &PageId) -> Option<Arc<Page>> {
        lock(&self.pages).iter().find(|p| p.id == *id).cloned()
    }

    fn take(&self, matches: impl Fn(&Page) -> bool) -> Option<Arc<Page>> {
        let mut pages = lock(&self.pages);
        let at = pages.iter().position(|p| matches(p))?;
        Some(pages.remove(at))
    }

    /// Opens a page at `url` with a viewport of `size`.
    pub(crate) async fn open(&self, id: PageId, url: &str, size: (u32, u32)) -> Result<PageInfo> {
        self.creating.send_modify(|n| *n += 1);
        let created = self
            .cdp
            .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
            .await
            .and_then(|created| text(&created, "targetId"));
        if let Ok(target) = &created {
            lock(&self.own).insert(target.clone());
        }
        self.creating.send_modify(|n| *n -= 1);
        let target = created?;
        let attach = json!({ "targetId": target, "flatten": true });
        let attached = self.cdp.call(None, "Target.attachToTarget", attach).await?;
        let page = Arc::new(Page::new(
            id,
            self.id.clone(),
            target.clone(),
            text(&attached, "sessionId")?,
            self.cdp.clone(),
        ));
        lock(&self.pages).push(page.clone());
        let ready = match page.prepare(size).await {
            Ok(()) => page.navigate(url).await,
            Err(e) => Err(e),
        };
        if let Err(e) = ready {
            self.take(|p| p.id == page.id);
            let close = json!({ "targetId": target });
            let _ = self.cdp.call(None, "Target.closeTarget", close).await;
            return Err(e);
        }
        let now = self
            .cdp
            .call(None, "Target.getTargetInfo", json!({ "targetId": target }))
            .await
            .unwrap_or_default();
        let now = &now["targetInfo"];
        page.retitle(
            now["url"].as_str().unwrap_or(url),
            now["title"].as_str().unwrap_or_default(),
        );
        page.opened();
        let info = page.info();
        self.sink.page(info.clone());
        Ok(info)
    }

    /// Closes one page.
    pub(crate) fn close(&self, id: &PageId) -> bool {
        let Some(page) = self.take(|p| p.id == *id) else {
            return false;
        };
        let close = json!({ "targetId": page.target });
        self.cdp.send(None, "Target.closeTarget", close);
        self.sink.closed(&page.id);
        true
    }

    /// Reports every page closed and stops the browser.
    pub(crate) fn end(&self) {
        let pages = std::mem::take(&mut *lock(&self.pages));
        for page in pages {
            self.sink.closed(&page.id);
        }
        self.kill();
    }

    /// Stops the browser and removes its profile: at once, and again once
    /// it has exited, for what it wrote while dying.
    fn kill(&self) {
        let child = lock(&self.child).take();
        let dir = lock(&self.profile_dir).take();
        let remove = move || {
            if let Some(dir) = &dir {
                let _ = std::fs::remove_dir_all(dir);
            }
        };
        remove();
        if let Some(mut child) = child {
            let _ = child.start_kill();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = child.wait().await;
                    remove();
                });
            }
        }
    }

    fn event(&self, event: &Event) {
        if let Some(cdp_session) = &event.session {
            let page = lock(&self.pages)
                .iter()
                .find(|p| p.cdp_session == *cdp_session)
                .cloned();
            if let Some(page) = page {
                page.event(event, &self.sink);
            }
            return;
        }
        let info = &event.params["targetInfo"];
        match event.method.as_str() {
            "Target.targetInfoChanged" => {
                let target = info["targetId"].as_str().unwrap_or_default();
                let page = lock(&self.pages)
                    .iter()
                    .find(|p| p.target == target)
                    .cloned();
                let url = info["url"].as_str().unwrap_or_default();
                let title = info["title"].as_str().unwrap_or_default();
                if let Some(page) = page
                    && page.retitle(url, title)
                    && page.is_open()
                {
                    self.sink.page(page.info());
                }
            }
            "Target.targetDestroyed" | "Target.detachedFromTarget" => {
                let params = &event.params;
                let gone = self.take(|p| {
                    params["targetId"].as_str() == Some(&p.target)
                        || params["sessionId"].as_str() == Some(&p.cdp_session)
                });
                if let Some(page) = gone {
                    self.sink.closed(&page.id);
                }
            }
            "Target.attachedToTarget" => self.attached(&event.params),
            _ => {}
        }
    }

    /// A new target, waiting for the debugger. A page `open` did not make
    /// (a popup, `noopener` or not, a `target=_blank` link) is not watched,
    /// so it is closed before it runs or sends a request; whether it is
    /// one of ours is known once no `Target.createTarget` is waiting.
    /// Service and shared workers run: they navigate nothing.
    fn attached(&self, params: &Value) {
        let info = &params["targetInfo"];
        let (Some(target), Some(session)) =
            (info["targetId"].as_str(), params["sessionId"].as_str())
        else {
            return;
        };
        let (cdp, session) = (self.cdp.clone(), session.to_owned());
        let run = move || cdp.send(Some(&session), "Runtime.runIfWaitingForDebugger", json!({}));
        if info["type"] != "page" {
            run();
            return;
        }
        let (cdp, own, target) = (self.cdp.clone(), self.own.clone(), target.to_owned());
        let decide = move || {
            if lock(&own).contains(&target) {
                run();
            } else {
                cdp.send(None, "Target.closeTarget", json!({ "targetId": target }));
            }
        };
        let mut creating = self.creating.subscribe();
        if *creating.borrow() == 0 {
            decide();
        } else {
            tokio::spawn(async move {
                let _ = creating.wait_for(|n| *n == 0).await;
                decide();
            });
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

fn text(value: &Value, key: &str) -> Result<String> {
    value[key].as_str().map(str::to_owned).ok_or_else(|| {
        ProtocolError::new(ErrorCode::Internal, format!("the browser sent no {key}"))
    })
}

async fn dispatch(
    session: Weak<Session>,
    mut events: mpsc::UnboundedReceiver<Event>,
    sessions: Sessions,
) {
    while let Some(event) = events.recv().await {
        let Some(session) = session.upgrade() else {
            return;
        };
        session.event(&event);
    }
    // The browser closed or crashed.
    if let Some(session) = session.upgrade() {
        let mut all = lock(&sessions);
        if all
            .get(&session.id)
            .is_some_and(|s| Arc::ptr_eq(s, &session))
        {
            all.remove(&session.id);
        }
        drop(all);
        session.end();
    }
}
