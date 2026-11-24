use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::fakes::{MemoryFs, MemoryProjects, RecordingOpener};
use crate::protocol::fs::FileBody;
use crate::protocol::info::InfoEvent;
use crate::protocol::projects::{Project, ProjectState};

struct Fixture {
    core: Core,
    fs: Arc<MemoryFs>,
    opener: Arc<RecordingOpener>,
}

fn fixture() -> Fixture {
    let fs = Arc::new(MemoryFs::default());
    let opener = Arc::new(RecordingOpener::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .opener(opener.clone())
        .build();
    Fixture { core, fs, opener }
}

fn req(json: serde_json::Value) -> Request {
    serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request in test: {e}"))
}

async fn add(core: &Core, root: &str) -> Project {
    match core
        .handle(req(
            serde_json::json!({"method": "projects.add", "params": {"root": root}}),
        ))
        .await
    {
        Ok(Response::Projects(ProjectsResponse::Project(p))) => p,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn info_reports_mode_and_version() {
    let core = Core::new("test");
    let reply = core
        .handle(req(serde_json::json!({"method": "info.get"})))
        .await;
    let Ok(Response::Info(InfoResponse::Info(info))) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!((info.name.as_str(), info.mode.as_str()), ("ovra", "test"));
}

#[tokio::test]
async fn unwired_services_are_unavailable() {
    let core = Core::new("test");
    let reply = core
        .handle(req(serde_json::json!({"method": "projects.list"})))
        .await;
    assert_eq!(reply.err().map(|e| e.code), Some(ErrorCode::Unavailable));
}

#[tokio::test]
async fn open_switches_the_watched_project_and_emits() {
    let f = fixture();
    let mut events = f.core.subscribe();
    let a = add(&f.core, "/work/a").await;
    let b = add(&f.core, "/work/b").await;
    for id in [&a.id, &b.id] {
        let r = f
            .core
            .handle(Request::Projects(ProjectsRequest::Open { id: id.clone() }))
            .await;
        assert!(r.is_ok(), "{r:?}");
    }
    assert_eq!(f.fs.watched_ids(), vec![b.id.clone()]);
    let mut last = None;
    while let Ok(event) = events.try_recv() {
        last = Some(event);
    }
    assert_eq!(
        last,
        Some(Event::Projects(ProjectsEvent::Changed {
            active: Some(b.id)
        }))
    );
}

#[tokio::test]
async fn relative_roots_are_rejected() {
    let f = fixture();
    let r = f
        .core
        .handle(Request::Projects(ProjectsRequest::Add {
            root: "rel".into(),
        }))
        .await;
    assert_eq!(r.err().map(|e| e.code), Some(ErrorCode::BadRequest));
}

#[tokio::test]
async fn project_state_is_saved_normalized_and_kept_inside() {
    let f = fixture();
    let p = add(&f.core, "/work/a").await;
    let get = |id: ProjectId| {
        f.core
            .handle(Request::Projects(ProjectsRequest::GetState { id }))
    };
    let set = |state: serde_json::Value| {
        f.core.handle(req(serde_json::json!({
            "method": "projects.set_state",
            "params": {"id": p.id.0, "state": state},
        })))
    };
    let state = |open: &[&str], active: Option<&str>, dirs: &[&str]| {
        Ok(Response::Projects(ProjectsResponse::State(ProjectState {
            open_files: open.iter().map(|s| (*s).to_owned()).collect(),
            active_file: active.map(str::to_owned),
            expanded_dirs: dirs.iter().map(|s| (*s).to_owned()).collect(),
        })))
    };
    assert_eq!(get(p.id.clone()).await, state(&[], None, &[]));

    // Panels save only their own fields, normalized.
    let saved = set(serde_json::json!({"expanded_dirs": ["src/"]})).await;
    assert_eq!(saved, Ok(Response::Ok(OkResponse::Ok)));
    set(serde_json::json!({"open_files": ["./src//main.rs", "README.md"], "active_file": "README.md"}))
        .await
        .expect("save tabs");
    let tabs_and_tree = state(&["src/main.rs", "README.md"], Some("README.md"), &["src"]);
    assert_eq!(get(p.id.clone()).await, tabs_and_tree);
    set(serde_json::json!({})).await.expect("empty patch");
    assert_eq!(get(p.id.clone()).await, tabs_and_tree);
    // `null` clears the active file; a missing field would keep it.
    set(serde_json::json!({"active_file": null}))
        .await
        .expect("clear active");
    assert_eq!(
        get(p.id.clone()).await,
        state(&["src/main.rs", "README.md"], None, &["src"])
    );

    let escape = set(serde_json::json!({"active_file": "../b/secret"})).await;
    assert_eq!(escape.err().map(|e| e.code), Some(ErrorCode::Forbidden));
    let escape = set(serde_json::json!({"expanded_dirs": ["/etc"]})).await;
    assert!(escape.is_err(), "{escape:?}");
    let unknown = get(ProjectId("nope".into())).await;
    assert_eq!(unknown.err().map(|e| e.code), Some(ErrorCode::NotFound));
}

#[tokio::test]
async fn fs_requests_resolve_project_and_reject_escapes() {
    let f = fixture();
    let p = add(&f.core, "/work/a").await;
    f.fs.put(Path::new("/work/a"), "src/main.rs", "fn main() {}");

    let read = f
        .core
        .handle(Request::Fs(FsRequest::ReadFile {
            project: p.id.clone(),
            path: "./src//main.rs".into(),
        }))
        .await;
    let Ok(Response::Fs(FsResponse::File(file))) = read else {
        panic!("{read:?}")
    };
    assert_eq!(
        file.body,
        FileBody::Text {
            text: "fn main() {}".into()
        }
    );

    let escape = f
        .core
        .handle(Request::Fs(FsRequest::ReadFile {
            project: p.id,
            path: "../b/secret".into(),
        }))
        .await;
    assert_eq!(escape.err().map(|e| e.code), Some(ErrorCode::Forbidden));
}

#[tokio::test]
async fn write_conflicts_when_file_changed() {
    let f = fixture();
    let p = add(&f.core, "/w").await;
    f.fs.put(Path::new("/w"), "a.txt", "one");
    let write = |expected| {
        f.core.handle(Request::Fs(FsRequest::WriteFile {
            project: p.id.clone(),
            path: "a.txt".into(),
            text: "two".into(),
            expected_mtime_ms: Some(expected),
        }))
    };
    assert_eq!(
        write(999).await.err().map(|e| e.code),
        Some(ErrorCode::Conflict)
    );
}

#[tokio::test]
async fn open_external_passes_absolute_path() {
    let f = fixture();
    let p = add(&f.core, "/w").await;
    let r = f
        .core
        .handle(Request::Open(OpenRequest::External {
            project: p.id,
            path: "a/b.rs".into(),
            line: Some(3),
        }))
        .await;
    assert_eq!(r, Ok(Response::Ok(OkResponse::Ok)));
    let opened = f
        .opener
        .opened
        .lock()
        .map(|o| o.clone())
        .unwrap_or_default();
    assert_eq!(opened, vec![(PathBuf::from("/w/a/b.rs"), Some(3))]);
}

#[tokio::test]
async fn open_url_accepts_only_web_pages() {
    let f = fixture();
    let open = |url: &str| {
        f.core
            .handle(Request::Open(OpenRequest::Url { url: url.into() }))
    };
    for url in ["https://example.com/a?b#c", "HTTP://localhost:8080"] {
        assert_eq!(open(url).await, Ok(Response::Ok(OkResponse::Ok)), "{url}");
    }
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "vscode://open",
        "https://",
        "https:///etc/passwd",
        "https://a b",
        "https://a\nb",
        "-https://a",
        "",
    ] {
        let code = open(url).await.err().map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::BadRequest), "{url}");
    }
    let urls = f.opener.urls.lock().map(|u| u.clone()).unwrap_or_default();
    assert_eq!(urls, ["https://example.com/a?b#c", "HTTP://localhost:8080"]);
}

#[test]
fn events_reach_subscribers() {
    let core = Core::new("test");
    let mut rx = core.subscribe();
    core.emit(InfoEvent::Ready);
    assert_eq!(rx.try_recv().ok(), Some(Event::Info(InfoEvent::Ready)));
}

#[tokio::test]
async fn a_lagging_subscriber_gets_resync_and_keeps_receiving() {
    let core = Core::new("test");
    let mut rx = core.subscribe();
    for _ in 0..EVENT_CAPACITY + 3 {
        core.emit(InfoEvent::Ready);
    }
    assert_eq!(
        Core::next_event(&mut rx).await,
        Some(Event::Info(InfoEvent::Resync { skipped: 3 }))
    );
    assert_eq!(
        Core::next_event(&mut rx).await,
        Some(Event::Info(InfoEvent::Ready))
    );
}

#[test]
fn wire_format_is_stable() {
    let samples = serde_json::json!({
        "requests": [
            Request::Info(InfoRequest::Get),
            Request::Projects(ProjectsRequest::Open { id: ProjectId("p1".into()) }),
            Request::Fs(FsRequest::ListDir { project: ProjectId("p1".into()), path: "src".into() }),
            Request::Open(OpenRequest::External { project: ProjectId("p1".into()), path: "a.rs".into(), line: None }),
            Request::Open(OpenRequest::Url { url: "https://example.com".into() }),
        ],
        "responses": [
            Response::Ok(OkResponse::Ok),
            Response::Fs(FsResponse::Written { mtime_ms: 5 }),
            Response::Projects(ProjectsResponse::State(ProjectState {
                open_files: vec!["a.rs".into()],
                active_file: Some("a.rs".into()),
                expanded_dirs: vec!["src".into()],
            })),
        ],
        "events": [
            Event::Info(InfoEvent::Ready),
            Event::Info(InfoEvent::Resync { skipped: 3 }),
            Event::Projects(ProjectsEvent::Changed { active: None }),
        ],
    });
    insta::assert_json_snapshot!(samples);
}

#[test]
fn every_request_round_trips() {
    let requests = [
        serde_json::json!({"method": "info.get"}),
        serde_json::json!({"method": "projects.list"}),
        serde_json::json!({"method": "projects.add", "params": {"root": "/x"}}),
        serde_json::json!({"method": "projects.remove", "params": {"id": "p1"}}),
        serde_json::json!({"method": "projects.open", "params": {"id": "p1"}}),
        serde_json::json!({"method": "projects.get_state", "params": {"id": "p1"}}),
        serde_json::json!({"method": "projects.set_state", "params": {"id": "p1", "state": {"open_files": ["a"], "active_file": null, "expanded_dirs": []}}}),
        serde_json::json!({"method": "projects.set_state", "params": {"id": "p1", "state": {"expanded_dirs": ["src"]}}}),
        serde_json::json!({"method": "fs.list_dir", "params": {"project": "p1", "path": ""}}),
        serde_json::json!({"method": "fs.read_file", "params": {"project": "p1", "path": "a"}}),
        serde_json::json!({"method": "fs.write_file", "params": {"project": "p1", "path": "a", "text": "", "expected_mtime_ms": null}}),
        serde_json::json!({"method": "open.external", "params": {"project": "p1", "path": "a", "line": 1}}),
        serde_json::json!({"method": "open.url", "params": {"url": "https://example.com"}}),
    ];
    for json in requests {
        let parsed: Request = req(json.clone());
        assert_eq!(serde_json::to_value(&parsed).ok(), Some(json));
    }
}
