//! The real Chromium, opt-in: `just browser-live [path]`, or
//! `OVRA_BROWSER_LIVE=<chromium> cargo test -p ovra-browser --test live -- --ignored`
//! (no path: the browser on `PATH`).
//! It runs without the Ovra sandbox (a pass-through one), against a page
//! served on loopback, to prove the pipe, the screencast, input, snapshots
//! and picks against the browser itself.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ovra_browser::ChromiumBrowser;
use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::browser::{FrameMetadata, MouseButton, PageId, PageInfo};
use ovra_core::services::{
    BoxFuture, Browser, BrowserSink, CommandLine, ElementRef, PageAction, ProfileNetwork, Result,
    Sandbox, SandboxProfile, SandboxStatus,
};

const PAGE: &str = r#"<!doctype html><title>Live</title>
<main><h1>Agent browser</h1>
<button id="save" class="btn primary" data-source="src/Save.tsx:12" style="color: rgb(255, 0, 0)"
  onclick="this.textContent = 'Saved'; console.log('clicked', 1)">Save</button>
<input aria-label="Email"></main>"#;

#[derive(Debug)]
struct PassThrough;

impl Sandbox for PassThrough {
    fn status(&self) -> SandboxStatus {
        SandboxStatus::Available {
            name: "none (live test)".into(),
            gaps: Vec::new(),
        }
    }
    fn wrap(&self, _: &SandboxProfile, line: CommandLine) -> Result<CommandLine> {
        Ok(line)
    }
}

#[derive(Debug, Default)]
struct Sink {
    frames: Mutex<Vec<FrameMetadata>>,
    asked: Mutex<Vec<String>>,
}

impl BrowserSink for Sink {
    fn frame(&self, _: &PageId, jpeg: String, metadata: FrameMetadata) {
        assert!(jpeg.starts_with("/9j/"), "a JPEG in base64");
        self.frames.lock().expect("frames").push(metadata);
    }
    fn page(&self, _: PageInfo) {}
    fn closed(&self, _: &PageId) {}
    fn navigation(&self, _: &PageId, origin: String) -> BoxFuture<'_, bool> {
        self.asked.lock().expect("asked").push(origin);
        Box::pin(async { true })
    }
}

/// Serves [`PAGE`] to every request on a loopback port.
fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let body = PAGE.as_bytes();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let mut stream = reader.into_inner();
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    port
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "no {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs the real Chromium: just browser-live"]
async fn the_real_chromium_over_the_pipe() {
    // Empty or unset: the browser on `PATH`.
    let chromium = std::env::var_os("OVRA_BROWSER_LIVE").filter(|p| !p.is_empty());
    let browser = ChromiumBrowser::new(Arc::new(PassThrough), chromium.map(Into::into));
    assert!(matches!(browser.status(), SandboxStatus::Available { .. }));
    let work = std::env::temp_dir();
    let profile = SandboxProfile {
        work_dir: work,
        read: Vec::new(),
        write: Vec::new(),
        expose: Vec::new(),
        forward: Vec::new(),
        network: ProfileNetwork::Blocked,
    };
    let sink = Arc::new(Sink::default());
    let url = format!("http://127.0.0.1:{}/", serve());
    let session = SessionId("live".into());
    let started = std::time::Instant::now();
    let sink_dyn: Arc<dyn BrowserSink> = sink.clone();
    let page = browser
        .open(
            session.clone(),
            profile,
            url.clone(),
            Some((800, 600)),
            sink_dyn,
        )
        .await
        .expect("open");
    eprintln!("opened in {:?}: {page:?}", started.elapsed());
    assert_eq!(page.title, "Live");
    assert_eq!(
        sink.asked.lock().expect("asked").as_slice(),
        [url.trim_end_matches('/')]
    );
    let page = page.id;

    browser.screencast(&page, true).expect("screencast");
    until("frame", || !sink.frames.lock().expect("frames").is_empty()).await;
    eprintln!("first frame: {:?}", sink.frames.lock().expect("frames")[0]);

    let snapshot = browser.snapshot(&page, None, None).await.expect("snapshot");
    eprintln!("{snapshot}");
    let button = snapshot
        .lines()
        .find(|l| l.contains("button \"Save\""))
        .and_then(|l| l.split("[ref=").nth(1))
        .map(|r| r.trim_end_matches(']').to_owned())
        .expect("a ref for the button");
    let click = PageAction::Click {
        target: button,
        button: MouseButton::Left,
        double: false,
        modifiers: 0,
    };
    browser.act(&page, click).await.expect("click");
    let wait = PageAction::WaitFor {
        text: Some("Saved".into()),
        text_gone: None,
        timeout: Duration::from_secs(5),
    };
    browser.act(&page, wait).await.expect("saved");
    let console = browser.console(&page, 0, 10).expect("console");
    assert!(console.iter().any(|e| e.text == "clicked 1"), "{console:?}");
    let network = browser.network(&page, 0, 10).expect("network");
    assert!(
        network
            .iter()
            .any(|e| e.url == url && e.status == Some(200)),
        "{network:?}"
    );

    let pick = browser.pick_at(&page, 0, 0, 0).await;
    eprintln!("pick at 0,0: {:?}", pick.map(|p| p.tag));
    let snapshot = browser.snapshot(&page, None, None).await.expect("snapshot");
    let saved = snapshot
        .lines()
        .find(|l| l.contains("button \"Saved\""))
        .and_then(|l| l.split("[ref=").nth(1))
        .map(|r| r.trim_end_matches(']').to_owned())
        .expect("the button again");
    let inspected = browser
        .inspect(&page, ElementRef::Ref(saved))
        .await
        .expect("inspect");
    eprintln!("inspected: {:#?}", inspected.element);
    assert!(inspected.png.starts_with(b"\x89PNG"));
    let rect = inspected.element.rect;
    let pick = browser
        .pick_at(&page, rect.x + rect.width / 2, rect.y + rect.height / 2, 0)
        .await
        .expect("pick");
    assert_eq!(
        (pick.tag.as_str(), pick.locator.as_str()),
        ("button", "button \"Saved\"")
    );
    assert_eq!(pick.id.as_deref(), Some("save"));
    assert_eq!(
        pick.styles.get("color").map(String::as_str),
        Some("rgb(255, 0, 0)")
    );
    assert_eq!(pick.source.and_then(|s| s.line), Some(12));
    let parent = browser
        .pick_at(&page, rect.x + 1, rect.y + 1, 1)
        .await
        .expect("parent");
    assert_eq!(parent.tag, "main");

    let shot = browser
        .screenshot(&page, None, true)
        .await
        .expect("screenshot");
    assert!(shot.starts_with(b"\x89PNG"));
    browser.end(&session);
    eprintln!(
        "{} frames; all done in {:?}",
        sink.frames.lock().expect("frames").len(),
        started.elapsed()
    );
}
