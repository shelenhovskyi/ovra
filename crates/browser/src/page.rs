//! One page of a session's browser, driven through its CDP session.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::browser::{FrameMetadata, InputEvent, PageId, PageInfo};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{BrowserSink, PageAction, Result};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::cdp::{Cdp, Event};
use crate::input;
use crate::logs::Logs;
use crate::origin::origin;
use crate::snapshot::{self, Refs};

mod element;

use element::stale;

/// A navigation may wait this long, for the person to answer the ask.
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(300);
/// How long a navigation waits for the page to load.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// At most 10 frames a second.
const FRAME_GAP: Duration = Duration::from_millis(100);
/// How often `wait_for` reads the page's text.
const POLL: Duration = Duration::from_millis(100);
/// Picks kept per page for `inspect`; the overlay picks on every hover.
const PICKS_KEPT: usize = 100;

/// The page's text, read in Ovra's isolated world.
const TEXT: &str = "document.body ? document.body.innerText : ''";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn error(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(code, message)
}

#[derive(Debug, Default)]
struct State {
    /// Whether `open` has answered with it.
    open: bool,
    url: String,
    title: String,
    screencast: bool,
    /// When the next screencast frame may be acknowledged.
    next_ack: Option<Instant>,
    logs: Logs,
    refs: Refs,
    picks: VecDeque<(String, i64)>,
    /// Ovra's isolated world in the main frame, while it lasts.
    world: Option<i64>,
}

#[derive(Debug)]
pub(crate) struct Page {
    pub(crate) id: PageId,
    pub(crate) owner: SessionId,
    /// The CDP target and the session attached to it.
    pub(crate) target: String,
    pub(crate) cdp_session: String,
    cdp: Cdp,
    state: Mutex<State>,
    /// Loads and same-document navigations of the main frame, counted.
    loads: watch::Sender<u64>,
}

impl Page {
    pub(crate) fn new(
        id: PageId,
        owner: SessionId,
        target: String,
        cdp_session: String,
        cdp: Cdp,
    ) -> Self {
        Self {
            id,
            owner,
            target,
            cdp_session,
            cdp,
            state: Mutex::default(),
            loads: watch::channel(0).0,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.cdp.call(Some(&self.cdp_session), method, params).await
    }

    fn send(&self, method: &str, params: Value) {
        self.cdp.send(Some(&self.cdp_session), method, params);
    }

    pub(crate) fn info(&self) -> PageInfo {
        let state = lock(&self.state);
        PageInfo {
            id: self.id.clone(),
            session: self.owner.clone(),
            url: state.url.clone(),
            title: state.title.clone(),
        }
    }

    /// Marks it opened: from now on its changes are reported.
    pub(crate) fn opened(&self) {
        lock(&self.state).open = true;
    }

    pub(crate) fn is_open(&self) -> bool {
        lock(&self.state).open
    }

    /// Takes in a target's new URL and title; whether either changed.
    pub(crate) fn retitle(&self, url: &str, title: &str) -> bool {
        let mut state = lock(&self.state);
        let changed = state.url != url || state.title != title;
        state.url = url.to_owned();
        state.title = title.to_owned();
        changed
    }

    /// Enables what the page reports, sizes its viewport and holds every
    /// document request until the sink allows its origin.
    pub(crate) async fn prepare(&self, size: (u32, u32)) -> Result<()> {
        for domain in ["Page", "Runtime", "Log", "Network", "DOM"] {
            self.call(&format!("{domain}.enable"), json!({})).await?;
        }
        let document =
            json!({"urlPattern": "*", "resourceType": "Document", "requestStage": "Request"});
        self.call("Fetch.enable", json!({ "patterns": [document] }))
            .await?;
        let metrics =
            json!({"width": size.0, "height": size.1, "deviceScaleFactor": 1, "mobile": false});
        self.call("Emulation.setDeviceMetricsOverride", metrics)
            .await?;
        Ok(())
    }

    /// Handles an event of this page's CDP session.
    pub(crate) fn event(self: &Arc<Self>, event: &Event, sink: &Arc<dyn BrowserSink>) {
        let params = &event.params;
        match event.method.as_str() {
            "Page.screencastFrame" => self.frame(params, sink),
            "Fetch.requestPaused" => {
                let (page, sink) = (self.clone(), sink.clone());
                let request = params["requestId"].as_str().unwrap_or_default().to_owned();
                let url = params["request"]["url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                tokio::spawn(async move {
                    // What is not http or https is never asked about.
                    let allowed = match origin(&url) {
                        Some(origin) => sink.navigation(&page.id, origin).await,
                        None => false,
                    };
                    if allowed {
                        page.send("Fetch.continueRequest", json!({ "requestId": request }));
                    } else {
                        let failed =
                            json!({"requestId": request, "errorReason": "BlockedByClient"});
                        page.send("Fetch.failRequest", failed);
                    }
                });
            }
            "Page.loadEventFired" => self.loaded(),
            "Page.navigatedWithinDocument" if params["frameId"] == json!(self.target) => {
                self.loaded()
            }
            "Runtime.executionContextsCleared" => lock(&self.state).world = None,
            "Runtime.executionContextDestroyed" => {
                let mut state = lock(&self.state);
                if state.world.is_some() && state.world == params["executionContextId"].as_i64() {
                    state.world = None;
                }
            }
            method => lock(&self.state).logs.event(method, params),
        }
    }

    fn loaded(&self) {
        self.loads.send_modify(|n| *n += 1);
    }

    /// Hands a frame on and acknowledges it no sooner than [`FRAME_GAP`]
    /// after the last: Chromium sends the next one only then.
    fn frame(&self, params: &Value, sink: &Arc<dyn BrowserSink>) {
        let ack = json!({ "sessionId": params["sessionId"] });
        let due = {
            let mut state = lock(&self.state);
            if !state.screencast {
                return;
            }
            let now = Instant::now();
            let due = state.next_ack.map_or(now, |due| due.max(now));
            state.next_ack = Some(due + FRAME_GAP);
            due
        };
        let m = &params["metadata"];
        let number = |key: &str| m[key].as_f64().unwrap_or_default().round();
        let metadata = FrameMetadata {
            width: number("deviceWidth") as u32,
            height: number("deviceHeight") as u32,
            scroll_x: number("scrollOffsetX") as i32,
            scroll_y: number("scrollOffsetY") as i32,
        };
        let jpeg = params["data"].as_str().unwrap_or_default().to_owned();
        sink.frame(&self.id, jpeg, metadata);
        let (cdp, session) = (self.cdp.clone(), self.cdp_session.clone());
        tokio::spawn(async move {
            tokio::time::sleep_until(due).await;
            cdp.send(Some(&session), "Page.screencastFrameAck", ack);
        });
    }

    pub(crate) fn screencast(&self, on: bool) {
        lock(&self.state).screencast = on;
        if on {
            let params = json!({"format": "jpeg", "quality": 70, "everyNthFrame": 1});
            self.send("Page.startScreencast", params);
        } else {
            self.send("Page.stopScreencast", json!({}));
        }
    }

    pub(crate) async fn input(&self, events: &[InputEvent]) -> Result<()> {
        for event in events {
            let (method, params) = input::command(event);
            self.call(method, params).await?;
        }
        Ok(())
    }

    pub(crate) fn console(
        &self,
        since: u64,
        limit: usize,
    ) -> Vec<ovra_core::services::ConsoleEntry> {
        lock(&self.state).logs.console(since, limit)
    }

    pub(crate) fn network(
        &self,
        since: u64,
        limit: usize,
    ) -> Vec<ovra_core::services::NetworkEntry> {
        lock(&self.state).logs.network(since, limit)
    }

    /// Waits until the page loaded since `before`, for [`LOAD_TIMEOUT`] at
    /// most: a slow page is the agent's to wait for.
    async fn settle(&self, before: u64) {
        let mut loads = self.loads.subscribe();
        let _ = tokio::time::timeout(LOAD_TIMEOUT, loads.wait_for(|n| *n != before)).await;
    }

    pub(crate) async fn navigate(&self, url: &str) -> Result<()> {
        let shown = match origin(url) {
            Some(origin) => origin,
            None if url == "about:blank" => url.to_owned(),
            None => {
                return Err(error(
                    ErrorCode::BadRequest,
                    "the agent browser opens only http and https URLs",
                ));
            }
        };
        let before = *self.loads.borrow();
        let answer = self
            .cdp
            .call_for(
                Some(&self.cdp_session),
                "Page.navigate",
                json!({ "url": url }),
                NAVIGATION_TIMEOUT,
            )
            .await?;
        match answer["errorText"].as_str() {
            Some("net::ERR_BLOCKED_BY_CLIENT") => {
                return Err(error(
                    ErrorCode::Forbidden,
                    format!("{shown} was not allowed"),
                ));
            }
            Some(text) if !text.is_empty() => {
                return Err(error(
                    ErrorCode::Internal,
                    format!("could not load {url}: {text}"),
                ));
            }
            _ => {}
        }
        // A navigation within the document has no loader to wait for.
        if answer.get("loaderId").is_some() {
            self.settle(before).await;
        }
        Ok(())
    }

    async fn history(&self, step: i64) -> Result<()> {
        let history = self.call("Page.getNavigationHistory", json!({})).await?;
        let at = history["currentIndex"].as_i64().unwrap_or_default() + step;
        let entry = usize::try_from(at)
            .ok()
            .and_then(|at| history["entries"].as_array()?.get(at).cloned())
            .ok_or_else(|| {
                let way = if step < 0 { "back" } else { "forward" };
                error(
                    ErrorCode::BadRequest,
                    format!("there is no page to go {way} to"),
                )
            })?;
        let before = *self.loads.borrow();
        self.call(
            "Page.navigateToHistoryEntry",
            json!({ "entryId": entry["id"] }),
        )
        .await?;
        self.settle(before).await;
        Ok(())
    }

    fn node(&self, target: &str) -> Result<i64> {
        lock(&self.state).refs.resolve(target)
    }

    pub(crate) async fn act(&self, action: PageAction) -> Result<()> {
        match action {
            PageAction::Navigate { url } => self.navigate(&url).await,
            PageAction::Back => self.history(-1).await,
            PageAction::Forward => self.history(1).await,
            PageAction::Reload => {
                let before = *self.loads.borrow();
                self.call("Page.reload", json!({})).await?;
                self.settle(before).await;
                Ok(())
            }
            PageAction::Click {
                target,
                button,
                double,
                modifiers,
            } => {
                let (x, y) = self.center(self.node(&target)?).await?;
                self.input(&input::click(x, y, button, double, modifiers))
                    .await
            }
            PageAction::Type {
                target,
                text,
                submit,
            } => {
                let backend = self.node(&target)?;
                self.call("DOM.focus", json!({ "backendNodeId": backend }))
                    .await
                    .map_err(stale)?;
                // What it held is replaced, as a person selecting it all would.
                let mut select = json!({"type": "rawKeyDown", "key": "a", "code": "KeyA", "modifiers": 2, "windowsVirtualKeyCode": 65, "commands": ["selectAll"]});
                self.call("Input.dispatchKeyEvent", select.clone()).await?;
                select["type"] = json!("keyUp");
                self.call("Input.dispatchKeyEvent", select).await?;
                self.call("Input.insertText", json!({ "text": text }))
                    .await?;
                if submit {
                    self.input(&input::press("Enter")).await?;
                }
                Ok(())
            }
            PageAction::Press { key } => self.input(&input::press(&key)).await,
            PageAction::WaitFor {
                text,
                text_gone,
                timeout,
            } => self.wait_for(text, text_gone, timeout).await,
        }
    }

    async fn wait_for(
        &self,
        text: Option<String>,
        gone: Option<String>,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let world = self.world().await?;
            let read = json!({"expression": TEXT, "contextId": world, "returnByValue": true});
            let shown = self.call("Runtime.evaluate", read).await?;
            let shown = shown["result"]["value"].as_str().unwrap_or_default();
            let there = text.as_deref().is_none_or(|t| shown.contains(t));
            let away = gone.as_deref().is_none_or(|t| !shown.contains(t));
            if there && away {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let what = match (&text, &gone) {
                    (Some(t), _) if !there => format!("\"{t}\" to show"),
                    (_, Some(t)) => format!("\"{t}\" to go"),
                    _ => "the page".to_owned(),
                };
                return Err(error(
                    ErrorCode::Conflict,
                    format!("waited {} ms for {what}", timeout.as_millis()),
                ));
            }
            tokio::time::sleep(POLL.min(deadline - Instant::now())).await;
        }
    }

    pub(crate) async fn snapshot(
        &self,
        target: Option<String>,
        depth: Option<u32>,
    ) -> Result<String> {
        let root = target.map(|t| self.node(&t)).transpose()?;
        let tree = self.call("Accessibility.getFullAXTree", json!({})).await?;
        let nodes = tree["nodes"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        snapshot::render(nodes, root, depth, &mut lock(&self.state).refs)
    }
}
