//! The browser against a scripted CDP peer: no Chromium runs.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use base64::Engine as _;
use ovra_core::fakes::FakeSandbox;
use ovra_core::protocol::browser::{FrameMetadata, MouseButton};
use ovra_core::services::ProfileNetwork;
use serde_json::{Value, json};

use super::*;
use crate::cdp::peer::{Peer, Sent, pair};
use crate::launch::Launched;

#[derive(Debug, Default)]
struct Sink {
    log: Mutex<Vec<String>>,
    refuse: AtomicBool,
}

impl BrowserSink for Sink {
    fn frame(&self, page: &PageId, jpeg: String, m: FrameMetadata) {
        let at = tokio::time::Instant::now();
        lock(&self.log).push(format!(
            "frame {} {jpeg} {}x{} +{} at {at:?}",
            page.0, m.width, m.height, m.scroll_y
        ));
    }
    fn page(&self, info: PageInfo) {
        lock(&self.log).push(format!("page {} {} {:?}", info.id.0, info.url, info.title));
    }
    fn closed(&self, page: &PageId) {
        lock(&self.log).push(format!("closed {}", page.0));
    }
    fn navigation(&self, page: &PageId, origin: String) -> BoxFuture<'_, bool> {
        lock(&self.log).push(format!("ask {} {origin}", page.0));
        let allow = !self.refuse.load(Ordering::Relaxed);
        Box::pin(async move { allow })
    }
}

impl Sink {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.log))
    }
}

/// What the scripted Chromium was sent, shared with the test.
type Sents = Arc<Mutex<Vec<Sent>>>;

type Inject = tokio::sync::mpsc::UnboundedReceiver<Value>;

#[derive(Debug, Default, Clone)]
struct Script {
    sent: Sents,
    /// Ends the peer when set: the browser "crashes".
    crash: Arc<tokio::sync::Notify>,
    /// Makes a page open a popup while `Target.createTarget` runs.
    popup: Arc<AtomicBool>,
    /// Messages the peer writes when the test says so.
    inject: Arc<Mutex<Option<Inject>>>,
}

impl Launch for Script {
    fn launch(&self, _: &SandboxProfile, _: &std::path::Path) -> Result<Launched> {
        let (reader, writer, peer) = pair();
        tokio::spawn(chrome(peer, self.clone()));
        Ok(Launched {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: None,
            stderr: None,
        })
    }
}

/// A Chromium that knows one page: the tree of the snapshot tests, a
/// button at backend node 17, a text that reads "Ready" on the third look.
async fn chrome(mut peer: Peer, script: Script) {
    let Script {
        sent, crash, popup, ..
    } = &script;
    let mut inject = lock(&script.inject).take();
    let mut navigation: Option<(u64, String)> = None;
    let (mut targets, mut looks) = (0, 0);
    loop {
        let injected = async {
            match &mut inject {
                Some(inject) => inject.recv().await,
                None => std::future::pending().await,
            }
        };
        let message = tokio::select! {
            m = peer.next() => m,
            () = crash.notified() => None,
            Some(v) = injected => {
                peer.write(v).await;
                continue;
            }
        };
        let Some(m) = message else { return };
        lock(sent).push(m.clone());
        let s = m.session.as_deref();
        let answer = match m.method.as_str() {
            "Target.createTarget" => {
                targets += 1;
                // Before the answer, as Chromium does: the client cannot
                // know yet whether the target is its own.
                if popup.load(Ordering::Relaxed) {
                    peer.write(attached(&format!("P{targets}"), "page")).await;
                }
                peer.write(attached(&format!("T{targets}"), "page")).await;
                json!({ "targetId": format!("T{targets}") })
            }
            "Target.attachToTarget" => {
                json!({ "sessionId": format!("S{}", &m.params["targetId"].as_str().unwrap_or_default()[1..]) })
            }
            "Target.getTargetInfo" => {
                json!({"targetInfo": {"targetId": m.params["targetId"], "url": navigation.as_ref().map(|n| n.1.clone()), "title": "Demo"}})
            }
            "Target.closeTarget" => {
                peer.event(
                    None,
                    "Target.targetDestroyed",
                    json!({ "targetId": m.params["targetId"] }),
                )
                .await;
                json!({ "success": true })
            }
            "Page.navigate" => {
                let url = m.params["url"].as_str().unwrap_or_default().to_owned();
                peer.event(
                    s,
                    "Fetch.requestPaused",
                    json!({"requestId": "r1", "resourceType": "Document", "request": {"url": url}}),
                )
                .await;
                navigation = Some((m.id, url));
                continue;
            }
            "Fetch.continueRequest" | "Fetch.failRequest" => {
                peer.answer(m.id, json!({})).await;
                let Some((id, _)) = &navigation else { continue };
                if m.method == "Fetch.failRequest" {
                    peer.answer(*id, json!({"frameId": "T1", "loaderId": "L1", "errorText": "net::ERR_BLOCKED_BY_CLIENT"})).await;
                } else {
                    peer.answer(*id, json!({"frameId": "T1", "loaderId": "L1"}))
                        .await;
                    peer.event(s, "Page.loadEventFired", json!({"timestamp": 1}))
                        .await;
                }
                continue;
            }
            "Page.startScreencast" => {
                for n in 1..=2 {
                    let metadata = json!({"deviceWidth": 1280, "deviceHeight": 800, "scrollOffsetX": 0, "scrollOffsetY": 120});
                    peer.event(
                        s,
                        "Page.screencastFrame",
                        json!({"data": format!("/9j/{n}"), "sessionId": n, "metadata": metadata}),
                    )
                    .await;
                }
                json!({})
            }
            "Accessibility.getFullAXTree" => crate::snapshot::tests::tree(),
            "Accessibility.getPartialAXTree" => {
                json!({"nodes": [{"role": {"value": "button"}, "name": {"value": "Save changes"}}]})
            }
            "DOM.getNodeForLocation" => json!({ "backendNodeId": 18 }),
            "DOM.resolveNode" => json!({ "object": { "objectId": "o1" } }),
            "DOM.describeNode" => json!({ "node": { "backendNodeId": 17 } }),
            "DOM.getContentQuads" => json!({ "quads": [[10, 20, 90, 20, 90, 44, 10, 44]] }),
            "Page.createIsolatedWorld" => json!({ "executionContextId": 7 }),
            "Runtime.callFunctionOn" if m.params["returnByValue"] == json!(true) => {
                json!({ "result": { "type": "object", "value": crate::pick::tests::read() } })
            }
            "Runtime.callFunctionOn" => json!({ "result": { "type": "object", "objectId": "o2" } }),
            "Runtime.evaluate" => {
                looks += 1;
                json!({ "result": { "type": "string", "value": if looks >= 3 { "Ready" } else { "Loading" } } })
            }
            "Page.getLayoutMetrics" => {
                json!({"cssVisualViewport": {"pageX": 0, "pageY": 100}, "cssContentSize": {"width": 1280, "height": 3000}})
            }
            "Page.captureScreenshot" => {
                json!({ "data": base64::engine::general_purpose::STANDARD.encode("png!") })
            }
            _ => json!({}),
        };
        peer.answer(m.id, answer).await;
    }
}

/// The browser-level event of a new target waiting for the debugger, on
/// the session `A` + its id.
fn attached(target: &str, kind: &str) -> Value {
    json!({"method": "Target.attachedToTarget", "params": {
        "sessionId": format!("A{target}"), "waitingForDebugger": true,
        "targetInfo": {"targetId": target, "type": kind, "openerId": ""},
    }})
}

fn profile() -> SandboxProfile {
    SandboxProfile {
        work_dir: "/w".into(),
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    }
}

struct Setup {
    browser: ChromiumBrowser,
    sink: Arc<Sink>,
    sent: Sents,
    crash: Arc<tokio::sync::Notify>,
    popup: Arc<AtomicBool>,
    inject: tokio::sync::mpsc::UnboundedSender<Value>,
}

fn setup() -> Setup {
    let script = Script::default();
    let (inject, injected) = tokio::sync::mpsc::unbounded_channel();
    *lock(&script.inject) = Some(injected);
    let (sent, crash, popup) = (
        script.sent.clone(),
        script.crash.clone(),
        script.popup.clone(),
    );
    let status = SandboxStatus::Available {
        name: "script".into(),
        gaps: Vec::new(),
    };
    Setup {
        browser: ChromiumBrowser::with(Some(Box::new(script)), status),
        sink: Arc::default(),
        sent,
        crash,
        popup,
        inject,
    }
}

impl Setup {
    async fn open(&self, session: &str, url: &str) -> Result<PageInfo> {
        let sink: Arc<dyn BrowserSink> = self.sink.clone();
        self.browser
            .open(SessionId(session.into()), profile(), url.into(), None, sink)
            .await
    }

    /// The commands sent since the last look, as `method params`, without
    /// the ones every page sends while it opens.
    fn sent(&self, of: &[&str]) -> Vec<String> {
        std::mem::take(&mut *lock(&self.sent))
            .into_iter()
            .filter(|m| of.iter().any(|o| m.method.starts_with(o)))
            .map(|m| format!("{} {}", m.method, m.params))
            .collect()
    }

    /// Waits until a command sent so far, as `session method params`,
    /// contains `needle`; all of them, without taking them.
    async fn until(&self, needle: &str) -> Vec<String> {
        loop {
            let sent: Vec<String> = lock(&self.sent)
                .iter()
                .map(|m| {
                    let session = m.session.as_deref().unwrap_or_default();
                    format!("{session} {} {}", m.method, m.params)
                })
                .collect();
            if sent.iter().any(|m| m.contains(needle)) {
                return sent;
            }
            tokio::task::yield_now().await;
        }
    }
}

#[tokio::test]
async fn open_asks_the_sink_and_a_refused_origin_is_blocked() {
    let s = setup();
    let page = s.open("s1", "http://localhost:5173/").await.expect("open");
    assert_eq!((page.id.0.as_str(), page.title.as_str()), ("pg1", "Demo"));
    assert_eq!(
        s.sink.take(),
        [
            "ask pg1 http://localhost:5173",
            "page pg1 http://localhost:5173/ \"Demo\""
        ]
    );
    s.sink.refuse.store(true, Ordering::Relaxed);
    let url = PageAction::Navigate {
        url: "https://example.com/x".into(),
    };
    let refused = s
        .browser
        .act(&page.id, url)
        .await
        .err()
        .map(|e| (e.code, e.message));
    assert_eq!(
        refused,
        Some((
            ErrorCode::Forbidden,
            "https://example.com was not allowed".into()
        ))
    );
    let file = PageAction::Navigate {
        url: "file:///etc/passwd".into(),
    };
    assert_eq!(
        s.browser.act(&page.id, file).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    let second = s
        .open("s1", "https://example.com/")
        .await
        .err()
        .map(|e| e.code);
    assert_eq!(second, Some(ErrorCode::Forbidden));
    // One browser for the session; the refused page was closed again.
    let made = s.sent(&["Target.createTarget", "Target.closeTarget", "Fetch."]);
    insta::assert_debug_snapshot!(made);
    s.browser.end(&SessionId("s1".into()));
    assert_eq!(
        s.sink.take(),
        [
            "ask pg1 https://example.com",
            "ask pg2 https://example.com",
            "closed pg1"
        ]
    );
    assert_eq!(
        s.browser.console(&page.id, 0, 10).err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
}

#[tokio::test]
async fn only_pages_open_made_run_and_the_rest_close_before_they_load() {
    let s = setup();
    // A popup the page opens while `open` still waits for its target.
    s.popup.store(true, Ordering::Relaxed);
    s.open("s1", "http://localhost:5173/").await.expect("open");
    let sent = s.until(r#""P1""#).await;
    let auto =
        r#" Target.setAutoAttach {"autoAttach":true,"flatten":true,"waitForDebuggerOnStart":true}"#;
    assert!(sent.iter().any(|m| m == auto), "{sent:?}");
    // A `noopener` popup or a `target=_blank` link: no opener at all.
    for (target, kind) in [("T7", "page"), ("W1", "service_worker")] {
        let _ = s.inject.send(attached(target, kind));
    }
    s.until(r#""T7""#).await;
    let mut decided: Vec<String> = s
        .until("AW1 ")
        .await
        .into_iter()
        .filter(|m| m.contains("Target.closeTarget") || m.contains("runIfWaiting"))
        .collect();
    decided.sort();
    assert_eq!(
        decided,
        [
            r#" Target.closeTarget {"targetId":"P1"}"#,
            r#" Target.closeTarget {"targetId":"T7"}"#,
            "AT1 Runtime.runIfWaitingForDebugger {}",
            "AW1 Runtime.runIfWaitingForDebugger {}",
        ]
    );
}

#[tokio::test]
async fn frames_in_other_processes_hold_their_documents_before_they_run() {
    let s = setup();
    s.open("s1", "http://localhost:5173/").await.expect("open");
    s.sink.take();
    let below = |session: &str, child: &str, kind: &str| {
        json!({"method": "Target.attachedToTarget", "sessionId": session, "params": {
            "sessionId": child, "waitingForDebugger": true,
            "targetInfo": {"targetId": format!("I{child}"), "type": kind},
        }})
    };
    let paused = |session: &str, url: &str| {
        json!({"method": "Fetch.requestPaused", "sessionId": session, "params": {
            "requestId": "r9", "resourceType": "Document", "request": {"url": url},
        }})
    };
    // A cross-site frame, a frame inside it, and a worker of the page.
    for event in [
        below("S1", "F1", "iframe"),
        below("F1", "F2", "iframe"),
        below("S1", "D1", "worker"),
    ] {
        let _ = s.inject.send(event);
    }
    s.until("F2 Runtime.runIfWaitingForDebugger").await;
    s.until("D1 Runtime.runIfWaitingForDebugger").await;
    let _ = s.inject.send(paused("F2", "https://other.example/x"));
    s.until("F2 Fetch.continueRequest").await;
    s.sink.refuse.store(true, Ordering::Relaxed);
    let _ = s.inject.send(paused("F1", "https://third.example/"));
    let sent = s.until("F1 Fetch.failRequest").await;
    let s1 = r#"S1 Target.setAutoAttach {"autoAttach":true,"flatten":true,"waitForDebuggerOnStart":true}"#;
    assert!(sent.iter().any(|m| m == s1), "{sent:?}");
    // In order for each session; the sessions themselves interleave.
    let mut frames: Vec<&String> = sent
        .iter()
        .filter(|m| ["F1 ", "F2 ", "D1 "].iter().any(|f| m.starts_with(f)))
        .collect();
    frames.sort_by_key(|m| &m[..2]);
    insta::assert_debug_snapshot!(frames);
    assert_eq!(
        s.sink.take(),
        [
            "ask pg1 https://other.example",
            "ask pg1 https://third.example"
        ]
    );
}

#[tokio::test]
async fn downloads_are_denied_and_service_workers_bypassed() {
    let s = setup();
    s.open("s1", "http://localhost:5173/").await.expect("open");
    let sent = s.until("S1 Network.setBypassServiceWorker").await;
    let set: Vec<&String> = sent
        .iter()
        .filter(|m| m.contains("setDownloadBehavior") || m.contains("setBypassServiceWorker"))
        .collect();
    assert_eq!(
        set,
        [
            r#" Browser.setDownloadBehavior {"behavior":"deny"}"#,
            r#"S1 Network.setBypassServiceWorker {"bypass":true}"#,
        ]
    );
}

#[tokio::test]
async fn the_person_is_asked_about_the_origin_that_loads() {
    let s = setup();
    let page = s.open("s1", "http://localhost:5173/").await.expect("open");
    s.sink.take();
    let url = PageAction::Navigate {
        url: "http://localhost:5173@Evil.example:80/".into(),
    };
    s.browser.act(&page.id, url).await.expect("navigate");
    assert_eq!(s.sink.take()[0], "ask pg1 http://evil.example");
    let refused = PageAction::Navigate {
        url: "http://localhost:99999/".into(),
    };
    let code = s.browser.act(&page.id, refused).await.err().map(|e| e.code);
    assert_eq!(code, Some(ErrorCode::BadRequest));
}

#[tokio::test]
async fn snapshots_give_refs_that_clicks_and_typing_use() {
    let s = setup();
    let page = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    let text = s
        .browser
        .snapshot(&page, None, None)
        .await
        .expect("snapshot");
    assert!(
        text.contains("- button \"Save \\\"now\\\"\" [disabled] [ref=e5]"),
        "{text}"
    );
    s.sent(&[]);
    let click = PageAction::Click {
        target: "e5".into(),
        button: MouseButton::Left,
        double: false,
        modifiers: 0,
    };
    s.browser.act(&page, click).await.expect("click");
    let typing = PageAction::Type {
        target: "e4".into(),
        text: "me@x.y".into(),
        submit: true,
    };
    s.browser.act(&page, typing).await.expect("type");
    insta::assert_debug_snapshot!(s.sent(&["DOM.", "Input."]));
    s.browser
        .snapshot(&page, None, Some(1))
        .await
        .expect("again");
    let click = PageAction::Click {
        target: "e5".into(),
        button: MouseButton::Left,
        double: false,
        modifiers: 0,
    };
    let stale = s.browser.act(&page, click).await.err().map(|e| e.message);
    assert_eq!(
        stale.as_deref(),
        Some("e5 is not a ref of the latest snapshot: take a new snapshot")
    );
}

#[tokio::test]
async fn picks_and_inspections_carry_the_payload() {
    let s = setup();
    let page = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    s.sent(&[]);
    let pick = s.browser.pick_at(&page, 40, 30, 1).await.expect("pick");
    assert_eq!(
        (pick.pick_id.as_str(), pick.locator.as_str()),
        ("pk2", "button \"Save changes\"")
    );
    assert_eq!(pick.url, "http://localhost:5173/");
    let inspected = s
        .browser
        .inspect(&page, ElementRef::Pick("pk2".into()))
        .await
        .expect("inspect");
    assert_eq!(
        (inspected.element.pick_id.as_str(), inspected.png.as_slice()),
        ("pk2", b"png!".as_slice())
    );
    insta::assert_debug_snapshot!(s.sent(&[
        "DOM.getNodeForLocation",
        "DOM.resolveNode",
        "Page.captureScreenshot"
    ]));
    let gone = s
        .browser
        .inspect(&page, ElementRef::Pick("pk9".into()))
        .await
        .err()
        .map(|e| e.code);
    assert_eq!(gone, Some(ErrorCode::NotFound));
}

#[tokio::test(start_paused = true)]
async fn screencast_frames_reach_the_sink_and_are_acked_ten_a_second() {
    let s = setup();
    let page = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    s.sink.take();
    let start = tokio::time::Instant::now();
    s.browser.screencast(&page, true).expect("on");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let acks: Vec<Value> = std::mem::take(&mut *lock(&s.sent))
        .into_iter()
        .filter(|m| m.method == "Page.screencastFrameAck")
        .map(|m| m.params["sessionId"].clone())
        .collect();
    assert_eq!(acks, [json!(1), json!(2)]);
    let frames: Vec<String> = s.sink.take();
    assert_eq!(frames.len(), 2);
    assert!(
        frames[0].starts_with("frame pg1 /9j/1 1280x800 +120"),
        "{frames:?}"
    );
    s.browser.screencast(&page, false).expect("off");
    while s.sent(&["Page.stopScreencast"]).is_empty() {
        tokio::task::yield_now().await;
    }
    assert!(start.elapsed() >= Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn wait_for_reads_the_text_until_it_shows_or_time_runs_out() {
    let s = setup();
    let page = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    let wait = |text: &str, timeout: u64| PageAction::WaitFor {
        text: Some(text.into()),
        text_gone: Some("Loading".into()),
        timeout: Duration::from_millis(timeout),
    };
    let early = s
        .browser
        .act(&page, wait("Ready", 50))
        .await
        .err()
        .map(|e| e.message);
    assert_eq!(early.as_deref(), Some("waited 50 ms for \"Ready\" to show"));
    s.browser
        .act(&page, wait("Ready", 1000))
        .await
        .expect("shows");
    let looks = s.sent(&["Runtime.evaluate"]);
    assert_eq!(looks.len(), 3);
    assert!(looks[0].contains("\"contextId\":7"), "{looks:?}");
}

#[tokio::test]
async fn a_browser_that_goes_away_closes_its_pages() {
    let s = setup();
    let page = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    s.browser.close(&page).await.expect("close");
    assert_eq!(
        s.browser.close(&page).await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
    let again = s
        .open("s1", "http://localhost:5173/")
        .await
        .expect("open")
        .id;
    s.sink.take();
    s.crash.notify_one();
    while !lock(&s.browser.sessions).is_empty() {
        tokio::task::yield_now().await;
    }
    assert_eq!(s.sink.take(), [format!("closed {}", again.0)]);
}

#[test]
fn status_says_what_is_missing() {
    let reason = |browser: ChromiumBrowser| match browser.status() {
        SandboxStatus::Unavailable { reason } => reason,
        SandboxStatus::Available { name, .. } => name,
    };
    let unavailable = FakeSandbox {
        status: SandboxStatus::Unavailable {
            reason: "no bwrap".into(),
        },
        ..FakeSandbox::available()
    };
    let none = reason(ChromiumBrowser::new(Arc::new(unavailable), None));
    assert_eq!(
        none,
        "the agent browser runs only in the sandbox, which is unavailable: no bwrap"
    );
    let missing = reason(ChromiumBrowser::new(
        Arc::new(FakeSandbox::available()),
        Some("/no/chromium".into()),
    ));
    assert_eq!(missing, "no browser at /no/chromium");
    let shell = reason(ChromiumBrowser::new(
        Arc::new(FakeSandbox::available()),
        Some("/bin/sh".into()),
    ));
    assert_eq!(shell, "Chromium at /bin/sh");
}

#[cfg(unix)]
#[test]
fn profile_folders_get_random_private_names() {
    use std::os::unix::fs::PermissionsExt as _;
    let parent = std::env::temp_dir();
    let dirs = [
        profile_dir(&parent).expect("one"),
        profile_dir(&parent).expect("two"),
    ];
    let modes: Vec<Option<u32>> = dirs
        .iter()
        .map(|d| {
            let mode = d.metadata().ok().map(|m| m.permissions().mode() & 0o777);
            let _ = std::fs::remove_dir(d);
            mode
        })
        .collect();
    assert_eq!(modes, [Some(0o700), Some(0o700)]);
    let names: Vec<String> = dirs
        .iter()
        .map(|d| {
            d.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_ne!(names[0], names[1]);
    for name in &names {
        let random = name.strip_prefix("ovra-browser-").unwrap_or_default();
        let hex = random.len() == 32 && random.chars().all(|c| c.is_ascii_hexdigit());
        assert!(hex, "{name}");
    }
}
