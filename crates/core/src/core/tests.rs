use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::fakes::{MemoryFs, MemoryProjects, RecordingOpener};
use crate::protocol::agents::{
    AgentMessage, AgentSetup, AgentStatus, AgentsEvent, AgentsResponse, ConfigItem, ConfigKind,
    ProjectAgentConfig, ProjectConfigReview, SessionId, SetupState,
};
use crate::protocol::fs::{FileBody, FsEvent};
use crate::protocol::info::InfoEvent;
use crate::protocol::orchestration::{OrchestrationEvent, ShowTarget};
use crate::protocol::permissions::{
    PermissionId, PermissionReason, PermissionRequest, PermissionsEvent,
};
use crate::protocol::projects::{Project, ProjectState};
use crate::protocol::review::{ReviewEvent, ReviewId};
use crate::protocol::terminals::{TerminalId, TerminalsEvent, TerminalsResponse};

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
    let a = add(&f.core, crate::test_path!("/work/a")).await;
    let b = add(&f.core, crate::test_path!("/work/b")).await;
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
        Some(Event::Projects(ProjectsEvent::Changed { open: vec![b.id] }))
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
    let p = add(&f.core, crate::test_path!("/work/a")).await;
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
    let p = add(&f.core, crate::test_path!("/work/a")).await;
    f.fs.put(
        Path::new(crate::test_path!("/work/a")),
        "src/main.rs",
        "fn main() {}",
    );

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
    let p = add(&f.core, crate::test_path!("/w")).await;
    f.fs.put(Path::new(crate::test_path!("/w")), "a.txt", "one");
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
    let p = add(&f.core, crate::test_path!("/w")).await;
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
    assert_eq!(
        opened,
        vec![(PathBuf::from(crate::test_path!("/w/a/b.rs")), Some(3))]
    );
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
            Response::Terminals(TerminalsResponse::Scrollback { data: "$ ls\r\n".into(), seq: 2 }),
            Response::Agents(AgentsResponse::Setup(AgentSetup {
                path: None,
                executable: Some("/usr/local/bin/claude".into()),
                version: Some("2.1.331".into()),
                state: SetupState::Outdated { minimum: "2.1.331".into() },
            })),
            Response::Agents(AgentsResponse::ProjectConfig(ProjectConfigReview {
                current: ProjectAgentConfig {
                    fingerprint: "9f2c".into(),
                    items: vec![ConfigItem {
                        path: ".claude/settings.json".into(),
                        kind: ConfigKind::Hooks,
                        entries: vec!["PreToolUse: ./lint.sh".into()],
                    }],
                },
                trusted: None,
            })),
            Response::Projects(ProjectsResponse::State(ProjectState {
                open_files: vec!["a.rs".into()],
                active_file: Some("a.rs".into()),
                expanded_dirs: vec!["src".into()],
            })),
        ],
        "events": [
            Event::Info(InfoEvent::Ready),
            Event::Info(InfoEvent::Resync { skipped: 3 }),
            Event::Projects(ProjectsEvent::Changed { open: vec![] }),
            Event::Fs(FsEvent::FilesChanged {
                project: ProjectId("p1".into()),
                added: vec!["b.rs".into()],
                removed: vec!["a.rs".into()],
                version: 7,
            }),
            Event::Agents(AgentsEvent::Status {
                session: SessionId("s1".into()),
                project: ProjectId("p1".into()),
                status: AgentStatus::Failed { message: "no model".into() },
            }),
            Event::Agents(AgentsEvent::Message {
                session: SessionId("s1".into()),
                message: AgentMessage::ToolUse {
                    id: "tool1".into(),
                    name: "Task".into(),
                    input: serde_json::json!({"prompt": "look"}),
                    parent: Some("tool0".into()),
                },
            }),
            Event::Permissions(PermissionsEvent::Requested(PermissionRequest {
                id: PermissionId("perm1".into()),
                session: SessionId("s1".into()),
                project: ProjectId("p1".into()),
                cwd: Some("/work/demo".into()),
                tool: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
                title: Some("List files".into()),
                reason: Some(PermissionReason {
                    text: "This command requires approval".into(),
                    kind: Some("other".into()),
                }),
            })),
            Event::Terminals(TerminalsEvent::Output { id: TerminalId("t1".into()), data: "$ ".into(), seq: 1 }),
            Event::Terminals(TerminalsEvent::Exited { id: TerminalId("t1".into()), code: None }),
            Event::Review(ReviewEvent::Changed { project: ProjectId("p1".into()), id: ReviewId("r1".into()) }),
            Event::Orchestration(OrchestrationEvent::Show {
                project: ProjectId("p1".into()),
                target: ShowTarget::File { path: "a.rs".into(), line: Some(3) },
            }),
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
        serde_json::json!({"method": "fs.list_files", "params": {"project": "p1", "offset": 2, "limit": 10}}),
        serde_json::json!({"method": "fs.write_file", "params": {"project": "p1", "path": "a", "text": "", "expected_mtime_ms": null}}),
        serde_json::json!({"method": "open.external", "params": {"project": "p1", "path": "a", "line": 1}}),
        serde_json::json!({"method": "open.url", "params": {"url": "https://example.com"}}),
        serde_json::json!({"method": "terminals.open", "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}}),
        serde_json::json!({"method": "terminals.input", "params": {"id": "t1", "data": "ls\r"}}),
        serde_json::json!({"method": "terminals.resize", "params": {"id": "t1", "size": {"cols": 100, "rows": 30}}}),
        serde_json::json!({"method": "terminals.close", "params": {"id": "t1"}}),
        serde_json::json!({"method": "terminals.scrollback", "params": {"id": "t1"}}),
        serde_json::json!({"method": "agents.list", "params": {"project": "p1"}}),
        serde_json::json!({"method": "agents.start", "params": {"project": "p1", "agent": "claude", "mode": "direct", "prompt": "hi"}}),
        serde_json::json!({"method": "agents.send", "params": {"session": "s1", "text": "more"}}),
        serde_json::json!({"method": "agents.stop", "params": {"session": "s1"}}),
        serde_json::json!({"method": "agents.transcript", "params": {"session": "s1"}}),
        serde_json::json!({"method": "agents.setup", "params": {"agent": "claude"}}),
        serde_json::json!({"method": "agents.set_path", "params": {"agent": "claude", "path": null}}),
        serde_json::json!({"method": "agents.project_config", "params": {"project": "p1"}}),
        serde_json::json!({"method": "projects.trust", "params": {"id": "p1", "trusted": true, "fingerprint": "9f2c"}}),
        serde_json::json!({"method": "projects.trust", "params": {"id": "p1", "trusted": false}}),
        serde_json::json!({"method": "permissions.pending"}),
        serde_json::json!({"method": "permissions.answer", "params": {"id": "perm1", "decision": "allow"}}),
        serde_json::json!({"method": "permissions.answer", "params": {"id": "perm1", "decision": "deny", "message": "no"}}),
        serde_json::json!({"method": "review.list", "params": {"project": "p1"}}),
        serde_json::json!({"method": "review.diff", "params": {"id": "r1", "path": "a.rs"}}),
        serde_json::json!({"method": "review.approve", "params": {"id": "r1", "commit": "fix: a"}}),
        serde_json::json!({"method": "review.approve", "params": {"id": "r1", "commit": null}}),
        serde_json::json!({"method": "review.reject", "params": {"id": "r1"}}),
        serde_json::json!({"method": "review.comment", "params": {"id": "r1", "text": "also b"}}),
        serde_json::json!({"method": "orchestration.start", "params": {"project": "p1", "prompt": "x", "limits": {"max_workers": 2, "budget_micro_usd": null}}}),
        serde_json::json!({"method": "orchestration.list", "params": {"project": "p1"}}),
        serde_json::json!({"method": "orchestration.stop", "params": {"task": "task1"}}),
    ];
    for json in requests {
        let parsed: Request = req(json.clone());
        assert_eq!(serde_json::to_value(&parsed).ok(), Some(json));
    }
}

#[tokio::test]
async fn list_files_pages_through_the_project() {
    let f = fixture();
    let project = f
        .core
        .handle(Request::Projects(ProjectsRequest::Add {
            root: crate::test_path!("/w").into(),
        }))
        .await;
    let Ok(Response::Projects(ProjectsResponse::Project(project))) = project else {
        panic!("add failed: {project:?}");
    };
    for path in ["a.rs", "b.rs", "src/c.rs"] {
        f.fs.put(Path::new(crate::test_path!("/w")), path, "");
    }
    let page = |offset, limit| {
        f.core.handle(Request::Fs(FsRequest::ListFiles {
            project: project.id.clone(),
            offset,
            limit,
        }))
    };
    let Ok(Response::Fs(FsResponse::Files(list))) = page(1, Some(5)).await else {
        panic!("list_files failed");
    };
    assert_eq!(
        (list.paths, list.total),
        (vec!["b.rs".to_owned(), "src/c.rs".to_owned()], 3)
    );
    let Ok(Response::Fs(FsResponse::Files(list))) = page(0, Some(u32::MAX)).await else {
        panic!("list_files failed");
    };
    assert_eq!(
        list.paths.len(),
        3,
        "an oversized limit is capped, not refused"
    );
}

#[tokio::test]
async fn fake_write_conflicts_when_the_file_is_gone() {
    let fs = MemoryFs::default();
    let write = fs.write_file(
        "/w".into(),
        RelPath::parse("a").expect("path"),
        "x".into(),
        Some(1),
    );
    let err = write.await.expect_err("conflict");
    assert_eq!(err.code, ErrorCode::Conflict);
}

#[tokio::test]
async fn each_client_keeps_its_project_watched_until_it_lets_go() {
    let f = fixture();
    let a = add(&f.core, crate::test_path!("/work/a")).await;
    let b = add(&f.core, crate::test_path!("/work/b")).await;
    async fn open(client: &Client, id: &ProjectId) -> Result<Response, ProtocolError> {
        client
            .handle(Request::Projects(ProjectsRequest::Open { id: id.clone() }))
            .await
    }
    let (one, two, three) = (f.core.connect(), f.core.connect(), f.core.connect());
    open(&one, &a.id).await.expect("open a");
    open(&two, &b.id).await.expect("open b");
    open(&three, &b.id).await.expect("open b again");
    let mut watched = f.fs.watched_ids();
    watched.sort();
    assert_eq!(
        watched,
        vec![a.id.clone(), b.id.clone()],
        "one watch per project"
    );

    let mut events = f.core.subscribe();
    drop(two);
    assert_eq!(f.fs.watched_ids().len(), 2, "b is still held by three");
    drop(three);
    assert_eq!(f.fs.watched_ids(), vec![a.id.clone()]);
    let last = std::iter::from_fn(|| events.try_recv().ok()).last();
    assert_eq!(
        last,
        Some(Event::Projects(ProjectsEvent::Changed {
            open: vec![a.id.clone()]
        }))
    );

    // Switching projects lets go of the previous one.
    open(&one, &b.id).await.expect("switch");
    assert_eq!(f.fs.watched_ids(), vec![b.id]);
}
