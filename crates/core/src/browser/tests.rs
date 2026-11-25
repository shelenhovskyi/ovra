use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::fakes::{
    FakeBrowser, FakeDevServers, FakeNetProxy, MemoryAudit, MemoryFs, MemoryProjects,
    ScriptedAgent, Step,
};
use crate::protocol::agents::{SandboxNetwork, SandboxReport};
use crate::protocol::audit::{AuditEvent, AuditRange};
use crate::protocol::browser::{BrowserEvent, BrowserResponse, PageInfo};
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::{ServerStatus, ServersResponse};
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::{AuditLog, ProfileNetwork};

const ROOT: &str = crate::test_path!("/work/a");

pub(super) struct Fx {
    pub(super) core: Core,
    pub(super) browser: Arc<FakeBrowser>,
    pub(super) dev: Arc<FakeDevServers>,
    pub(super) audit: Arc<MemoryAudit>,
    pub(super) agent: Arc<ScriptedAgent>,
}

/// Project `p1` defining server `web` on 5173, every service fake. Its
/// agents report a sandbox behind the netproxy when `sandboxed`.
pub(super) async fn fx(sandboxed: bool) -> Fx {
    let fs = Arc::new(MemoryFs::default());
    let servers = r#"{"servers": [{"name": "web", "command": ["pnpm", "dev"], "port": 5173}]}"#;
    fs.put(Path::new(ROOT), ".ovra/servers.json", servers);
    let agent = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("on it")]]));
    if sandboxed {
        *super::lock(&agent.sandbox) = Some(SandboxReport {
            name: "fake".into(),
            network: SandboxNetwork::Proxy,
            gaps: Vec::new(),
        });
    }
    let (browser, dev) = (Arc::default(), Arc::new(FakeDevServers::default()));
    let audit = Arc::new(MemoryAudit::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs)
        .agents(agent.clone())
        .netproxy(Arc::new(FakeNetProxy::default()))
        .dev_servers(dev.clone())
        .browser(Arc::clone(&browser) as Arc<dyn crate::services::Browser>)
        .audit(audit.clone())
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    ok(
        &core,
        json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "look"}}),
    )
    .await;
    Fx {
        core,
        browser,
        dev,
        audit,
        agent,
    }
}

pub(super) async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

pub(super) async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json).await.unwrap_or_else(|e| panic!("{e}"))
}

pub(super) async fn open(core: &Core, url: &str) -> Result<PageInfo, ProtocolError> {
    let open = json!({"method": "browser.open", "params": {"session": "s1", "url": url}});
    match call(core, open).await? {
        Response::Browser(BrowserResponse::Page(info)) => Ok(info),
        other => panic!("unexpected {other:?}"),
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, ProtocolError>) -> ErrorCode {
    match result {
        Err(e) => e.code,
        Ok(v) => panic!("expected an error, got {v:?}"),
    }
}

/// Waits for the first event `pick` takes.
async fn next<T>(
    rx: &mut tokio::sync::broadcast::Receiver<Event>,
    pick: impl Fn(Event) -> Option<T>,
) -> T {
    loop {
        let event = Core::next_event(rx)
            .await
            .unwrap_or_else(|| panic!("no core"));
        if let Some(found) = pick(event) {
            return found;
        }
    }
}

fn listed(event: Event) -> Option<Vec<String>> {
    match event {
        Event::Browser(BrowserEvent::Pages { pages, .. }) => {
            Some(pages.into_iter().map(|p| p.id.0).collect())
        }
        _ => None,
    }
}

/// The browser navigations the audit log holds, as JSON.
pub(super) fn navigations(f: &Fx) -> Vec<serde_json::Value> {
    let entries = f
        .audit
        .list(&ProjectId("p1".into()), AuditRange::default(), 100)
        .unwrap_or_default();
    entries
        .into_iter()
        .filter(|e| matches!(e.event, AuditEvent::BrowserNavigation { .. }))
        .map(|e| serde_json::to_value(e.event).unwrap_or_default())
        .collect()
}

async fn ready_web(f: &Fx) {
    let start = json!({"method": "servers.start", "params": {"project": "p1", "name": "web"}});
    let Response::Servers(ServersResponse::Server(web)) = ok(&f.core, start).await else {
        panic!("no server");
    };
    f.dev.report(&web.id, ServerStatus::Ready);
}

#[tokio::test]
async fn a_page_opens_behind_the_agents_netproxy_and_reports_to_every_front_end() {
    let f = fx(true).await;
    ready_web(&f).await;
    let mut rx = f.core.subscribe();
    // A granted server opens without asking.
    let page = open(&f.core, "http://localhost:5173/").await;
    let page = page.unwrap_or_else(|e| panic!("{e}")).id;
    assert_eq!(next(&mut rx, listed).await, ["pg1"]);
    // The browser shares the agent's netproxy and nothing of the project.
    let agent = super::lock(&f.agent.started)[0].sandbox.clone();
    let started = super::lock(&f.browser.started).clone();
    let [(session, profile)] = started.as_slice() else {
        panic!("{started:?}");
    };
    assert_eq!(session.0, "s1");
    assert_eq!(profile.network, agent.network);
    assert!(matches!(profile.network, ProfileNetwork::Proxy { .. }));
    let seen = std::iter::once(&profile.work_dir)
        .chain(&profile.read)
        .chain(&profile.write);
    assert!(seen.clone().all(|p| !p.starts_with(ROOT)));
    assert_eq!(
        profile.work_dir,
        Path::new(""),
        "the service puts its own folder there"
    );
    assert_eq!(seen.count(), 1);
    assert!(profile.expose.is_empty() && profile.forward.is_empty());

    ok(
        &f.core,
        json!({"method": "browser.screencast", "params": {"page": "pg1", "on": true}}),
    )
    .await;
    assert!(f.browser.frame(&page, "/9j/"));
    let jpeg = next(&mut rx, |e| match e {
        Event::Browser(BrowserEvent::Frame { jpeg, .. }) => Some(jpeg),
        _ => None,
    });
    assert_eq!(jpeg.await, "/9j/");
    let input = json!({"method": "browser.input", "params": {"page": "pg1", "event":
        {"kind": "mouse_move", "x": 1, "y": 2}}});
    ok(&f.core, input).await;
    assert_eq!(f.browser.page(&page).map(|p| p.inputs.len()), Some(1));

    ok(
        &f.core,
        json!({"method": "browser.close", "params": {"page": "pg1"}}),
    )
    .await;
    assert_eq!(next(&mut rx, listed).await, Vec::<String>::new());
    let closed = json!({"method": "browser.screencast", "params": {"page": "pg1", "on": false}});
    assert_eq!(code(call(&f.core, closed).await), ErrorCode::NotFound);
    assert!(navigations(&f).is_empty(), "granted servers are not asked");
}

#[tokio::test]
async fn only_a_session_behind_its_netproxy_gets_a_browser() {
    let f = fx(false).await;
    let refused = open(&f.core, "http://localhost:5173/").await;
    let message = refused.as_ref().err().map(|e| (e.code, e.message.clone()));
    assert_eq!(
        message,
        Some((
            ErrorCode::Unavailable,
            "the agent browser runs only for sessions whose agent runs in the sandbox behind \
             its own netproxy, and this session's agent does not"
                .to_owned()
        ))
    );
    assert!(super::lock(&f.browser.started).is_empty());

    let unwired = Core::new("test");
    let open =
        json!({"method": "browser.open", "params": {"session": "s1", "url": "http://x.test/"}});
    assert_eq!(code(call(&unwired, open).await), ErrorCode::Unavailable);
}

#[tokio::test]
async fn a_session_has_four_pages_at_most_and_its_end_closes_them() {
    let f = fx(true).await;
    ready_web(&f).await;
    for _ in 0..4 {
        assert!(open(&f.core, "http://127.0.0.1:5173/").await.is_ok());
    }
    assert_eq!(
        code(open(&f.core, "http://localhost:5173/").await),
        ErrorCode::Conflict
    );
    let bad = [("file:///etc/passwd", None), ("http://x.test/", Some(0))];
    for (url, width) in bad {
        let open = json!({"method": "browser.open", "params": {"session": "s1", "url": url, "width": width}});
        assert_eq!(
            code(call(&f.core, open).await),
            ErrorCode::BadRequest,
            "{url}"
        );
    }
    ok(
        &f.core,
        json!({"method": "browser.close", "params": {"page": "pg2"}}),
    )
    .await;

    // Not a granted server: refused.
    assert_eq!(
        code(open(&f.core, "https://example.com/").await),
        ErrorCode::Forbidden
    );
    let mut rx = f.core.subscribe();
    ok(
        &f.core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    assert_eq!(next(&mut rx, listed).await, Vec::<String>::new());
    assert!(super::lock(&f.browser.pages).is_empty());
    assert_eq!(
        code(open(&f.core, "http://localhost:5173/").await),
        ErrorCode::Conflict
    );
}
