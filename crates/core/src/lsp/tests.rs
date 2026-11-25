use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::uris;
use crate::Core;
use crate::fakes::{FakeLanguageServers, FakeServer, MemoryProjects};
use crate::protocol::agents::ProjectAgentConfig;
use crate::protocol::lsp::{KnownServer, LspEvent, LspResponse, ServerInfo, ServerState};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::ProjectStore;

const ROOT: &str = crate::test_path!("/work/a");

struct Test {
    core: Core,
    store: Arc<MemoryProjects>,
    servers: Arc<FakeLanguageServers>,
    events: tokio::sync::broadcast::Receiver<Event>,
}

async fn setup(servers: FakeLanguageServers, trusted: bool) -> Test {
    let store = Arc::new(MemoryProjects::default());
    let servers = Arc::new(servers);
    let core = Core::builder("test")
        .projects(store.clone())
        .lsp(servers.clone())
        .build();
    let add = json!({"method": "projects.add", "params": {"root": ROOT}});
    ok(call(&core, add).await);
    if trusted {
        let config = ProjectAgentConfig {
            fingerprint: "f".into(),
            items: Vec::new(),
        };
        ok(store.set_trusted(&ProjectId("p1".into()), Some(config)));
    }
    let events = core.subscribe();
    Test {
        core,
        store,
        servers,
        events,
    }
}

async fn call(core: &Core, json: Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

fn ok<T>(result: Result<T, ProtocolError>) -> T {
    result.unwrap_or_else(|e| panic!("{e}"))
}

fn code<T: std::fmt::Debug>(result: Result<T, ProtocolError>) -> ErrorCode {
    match result {
        Ok(r) => panic!("expected an error, got {r:?}"),
        Err(e) => e.code,
    }
}

async fn start(core: &Core, language: &str) -> Result<ServerInfo, ProtocolError> {
    let request = json!({"method": "lsp.start", "params": {"project": "p1", "language": language}});
    match call(core, request).await? {
        Response::Lsp(LspResponse::Server(info)) => Ok(info),
        other => panic!("{other:?}"),
    }
}

async fn send(core: &Core, message: Value) -> Result<Response, ProtocolError> {
    call(
        core,
        json!({"method": "lsp.send", "params": {"server": "l1", "message": message}}),
    )
    .await
}

fn server(test: &Test) -> FakeServer {
    test.servers
        .started()
        .pop()
        .unwrap_or_else(|| panic!("no server started"))
}

/// The `lsp.*` events so far.
fn events(test: &mut Test) -> Vec<LspEvent> {
    std::iter::from_fn(|| test.events.try_recv().ok())
        .filter_map(|e| match e {
            Event::Lsp(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn under_root(path: &str) -> String {
    uris::file_uri(&Path::new(ROOT).join(path))
}

#[tokio::test]
async fn an_untrusted_project_gets_no_server() {
    let test = setup(FakeLanguageServers::default(), false).await;
    let status = call(
        &test.core,
        json!({"method": "lsp.status", "params": {"project": "p1"}}),
    )
    .await;
    let Ok(Response::Lsp(LspResponse::Status(status))) = status else {
        panic!("{status:?}");
    };
    assert!(!status.trusted);
    assert_eq!(status.sandbox.as_deref(), Some("fake"));
    assert_eq!(status.known.len(), 1);
    assert_eq!(code(start(&test.core, "rust").await), ErrorCode::Forbidden);
    assert!(test.servers.started().is_empty());
}

#[tokio::test]
async fn a_trusted_project_gets_one_server_per_language() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    let info = ok(start(&test.core, "rust").await);
    assert_eq!(info.name, "fake-ls");
    assert_eq!(info.state, ServerState::Running);
    assert_eq!(info.sandbox.as_deref(), Some("fake"));
    assert_eq!(server(&test).spec.root, Path::new(ROOT));
    let again = ok(start(&test.core, "rust").await);
    assert_eq!(again.id, info.id);
    assert_eq!(test.servers.started().len(), 1);
    let states: Vec<_> = events(&mut test)
        .into_iter()
        .filter_map(|e| match e {
            LspEvent::Changed { server } => Some(server.state),
            LspEvent::Message { .. } => None,
        })
        .collect();
    assert_eq!(states, [ServerState::Running]);
    assert_eq!(code(start(&test.core, "go").await), ErrorCode::NotFound);
    assert_eq!(
        code(start(&test.core, "Rust; rm").await),
        ErrorCode::BadRequest
    );
}

#[tokio::test]
async fn at_most_four_servers_run() {
    let known = ["a", "b", "c", "d", "e"]
        .map(|language| KnownServer {
            language: language.into(),
            name: format!("{language}-ls"),
            path: Some("/bin/x".into()),
            install: String::new(),
        })
        .to_vec();
    let test = setup(FakeLanguageServers::new(known), true).await;
    for language in ["a", "b", "c", "d"] {
        ok(start(&test.core, language).await);
    }
    assert_eq!(code(start(&test.core, "e").await), ErrorCode::Unavailable);
    ok(call(
        &test.core,
        json!({"method": "lsp.stop", "params": {"server": "l1"}}),
    )
    .await);
    ok(start(&test.core, "e").await);
}

#[tokio::test]
async fn client_uris_become_files_under_the_root() {
    let test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "processId": 4242,
        "rootUri": "ovra:///",
        "rootPath": "/elsewhere",
        "workspaceFolders": [{"uri": "ovra:///", "name": "a"}],
    }});
    ok(send(&test.core, initialize).await);
    let open = json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": {"uri": "ovra:///src/my%20lib.rs", "languageId": "rust", "version": 1,
            "text": "// see file:///etc/passwd and ovra:///x"},
    }});
    ok(send(&test.core, open).await);
    let sent = server(&test).sent();
    let params = &sent[0]["params"];
    assert_eq!(params["processId"], Value::Null);
    assert_eq!(params["rootUri"], uris::file_uri(Path::new(ROOT)));
    assert_eq!(params["rootPath"], ROOT);
    assert_eq!(
        params["workspaceFolders"][0]["uri"],
        uris::file_uri(Path::new(ROOT))
    );
    let document = &sent[1]["params"]["textDocument"];
    assert_eq!(document["uri"], under_root("src/my lib.rs"));
    // Document text is never rewritten.
    assert_eq!(document["text"], "// see file:///etc/passwd and ovra:///x");
}

#[tokio::test]
async fn client_messages_that_leave_the_project_are_refused() {
    let test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    let with_uri = |uri: &str| {
        json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": {
            "textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}}})
    };
    for uri in [
        "file:///etc/passwd",
        "FILE:///etc/passwd",
        "ovra:///../secret",
        "ovra:///src/%2E%2E/%2E%2E/secret",
        "ovra://other/x",
    ] {
        let refused = send(&test.core, with_uri(uri)).await;
        assert!(
            matches!(refused, Err(ref e) if matches!(e.code, ErrorCode::BadRequest | ErrorCode::Forbidden)),
            "{uri}: {refused:?}"
        );
    }
    let edit = json!({"jsonrpc": "2.0", "id": 3, "method": "x", "params": {
        "edit": {"changes": {"file:///etc/passwd": []}}}});
    assert_eq!(code(send(&test.core, edit).await), ErrorCode::BadRequest);
    let response = json!({"jsonrpc": "2.0", "id": 9, "result": null});
    assert_eq!(
        code(send(&test.core, response).await),
        ErrorCode::BadRequest
    );
    let old = json!({"jsonrpc": "1.0", "method": "exit"});
    assert_eq!(code(send(&test.core, old).await), ErrorCode::BadRequest);
    assert!(server(&test).sent().is_empty());
    let gone = call(
        &test.core,
        json!({"method": "lsp.send", "params": {"server": "l9", "message": {}}}),
    );
    assert_eq!(code(gone.await), ErrorCode::NotFound);
}

#[tokio::test]
async fn server_uris_under_the_root_reach_the_client_as_ovra_uris() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    events(&mut test);
    let fake = server(&test);
    let outside = "file:///home/me/.rustup/lib/std/src/vec.rs";
    fake.say(
        json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
        "uri": under_root("src/main.rs"), "diagnostics": []}}),
    );
    fake.say(json!({"jsonrpc": "2.0", "id": 4, "result": [
        {"uri": outside, "range": {}},
        {"targetUri": under_root(""), "range": {}},
        {"changes": {under_root("a b.rs"): [{"newText": "file:///x"}]}},
    ]}));
    let messages: Vec<Value> = events(&mut test)
        .into_iter()
        .filter_map(|e| match e {
            LspEvent::Message { message, .. } => Some(message),
            LspEvent::Changed { .. } => None,
        })
        .collect();
    assert_eq!(messages[0]["params"]["uri"], "ovra:///src/main.rs");
    let result = &messages[1]["result"];
    assert_eq!(result[0]["uri"], outside);
    assert_eq!(result[1]["targetUri"], "ovra:///");
    assert_eq!(
        result[2]["changes"]["ovra:///a%20b.rs"][0]["newText"],
        "file:///x"
    );
}

#[tokio::test]
async fn the_core_answers_server_requests_and_the_client_never_sees_them() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    events(&mut test);
    let fake = server(&test);
    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "workspace/configuration",
            "params": {"items": [{"section": "a"}, {"section": "b"}]}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "workspace/applyEdit",
            "params": {"edit": {"changes": {under_root("a.rs"): []}}}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "window/showDocument",
            "params": {"uri": "https://example.com", "external": true}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "workspace/workspaceFolders"}),
        json!({"jsonrpc": "2.0", "id": 5, "method": "client/registerCapability", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "experimental/anything"}),
    ];
    for request in requests {
        fake.say(request);
    }
    assert!(events(&mut test).is_empty());
    let replies = fake.sent();
    assert_eq!(replies[0]["result"], json!([null, null]));
    assert_eq!(replies[1]["result"]["applied"], false);
    assert_eq!(replies[2]["result"]["success"], false);
    assert_eq!(
        replies[3]["result"][0]["uri"],
        uris::file_uri(Path::new(ROOT))
    );
    assert_eq!(replies[4]["result"], Value::Null);
    assert_eq!(replies[5]["error"]["code"], -32601);
    let ids: Vec<_> = replies.iter().map(|r| r["id"].clone()).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6].map(Value::from));
}

#[tokio::test]
async fn stopping_tells_the_server_to_exit_and_the_client_it_stopped() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    events(&mut test);
    ok(call(
        &test.core,
        json!({"method": "lsp.stop", "params": {"server": "l1"}}),
    )
    .await);
    let fake = server(&test);
    assert!(fake.stopped());
    assert_eq!(
        fake.sent().last().map(|m| m["method"].clone()),
        Some(json!("exit"))
    );
    let changed = events(&mut test);
    assert!(
        matches!(&changed[..], [LspEvent::Changed { server }] if server.state == ServerState::Stopped),
        "{changed:?}"
    );
    let again = call(
        &test.core,
        json!({"method": "lsp.stop", "params": {"server": "l1"}}),
    );
    assert_eq!(code(again.await), ErrorCode::NotFound);
}

#[tokio::test]
async fn a_server_that_dies_shows_as_failed() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    events(&mut test);
    server(&test).exit(Some(101), None);
    let changed = events(&mut test);
    let [LspEvent::Changed { server }] = &changed[..] else {
        panic!("{changed:?}");
    };
    assert_eq!(
        server.state,
        ServerState::Failed {
            reason: "the server exited with code 101".into()
        }
    );
    // A new one may start.
    ok(start(&test.core, "rust").await);
}

#[tokio::test]
async fn a_flood_of_large_messages_is_held_back_until_clients_read() {
    let mut test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    events(&mut test);
    let server = server(&test);
    let note = json!({"jsonrpc": "2.0", "method": "window/logMessage"});
    // Three 4 MiB messages are few events but 12 MiB; the fourth fills
    // the 16 MiB budget.
    for _ in 0..3 {
        server.say_sized(note.clone(), 4 << 20);
        assert!(!server.busy());
    }
    server.say_sized(note.clone(), 4 << 20);
    assert!(server.busy());
    assert_eq!(events(&mut test).len(), 4);
    assert!(!server.busy());
    // Small messages are held back by their count.
    for _ in 0..super::MAX_BACKLOG {
        server.say(note.clone());
    }
    assert!(server.busy());
    events(&mut test);
    assert!(!server.busy());
}

#[tokio::test]
async fn taking_trust_back_stops_the_projects_servers() {
    let test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    let untrust = json!({"method": "projects.trust", "params": {"id": "p1", "trusted": false}});
    ok(call(&test.core, untrust).await);
    assert!(server(&test).stopped());
    assert!(!ok(test.store.get(&ProjectId("p1".into()))).trusted);
    assert_eq!(test.servers.forgotten(), [ProjectId("p1".into())]);
    assert_eq!(code(start(&test.core, "rust").await), ErrorCode::Forbidden);
}

#[tokio::test]
async fn removing_a_project_stops_its_servers_and_forgets_their_caches() {
    let test = setup(FakeLanguageServers::default(), true).await;
    ok(start(&test.core, "rust").await);
    assert!(test.servers.forgotten().is_empty());
    let remove = json!({"method": "projects.remove", "params": {"id": "p1"}});
    ok(call(&test.core, remove).await);
    assert!(server(&test).stopped());
    assert_eq!(test.servers.forgotten(), [ProjectId("p1".into())]);
}

#[tokio::test]
async fn a_server_that_was_starting_as_its_project_was_removed_leaves_no_cache() {
    let test = setup(FakeLanguageServers::default(), true).await;
    let gate = test.servers.hold_starts();
    let core = test.core.clone();
    let starting = tokio::spawn(async move { start(&core, "rust").await });
    while test.core.lsp_servers().lock().servers.is_empty() {
        tokio::task::yield_now().await;
    }
    let remove = json!({"method": "projects.remove", "params": {"id": "p1"}});
    ok(call(&test.core, remove).await);
    assert_eq!(test.servers.forgotten(), [ProjectId("p1".into())]);
    gate.add_permits(1);
    let started = starting.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(code(started), ErrorCode::Internal);
    assert!(server(&test).stopped());
    // Forgotten again, after the service made the server's cache folder.
    assert_eq!(
        test.servers.forgotten(),
        [ProjectId("p1".into()), ProjectId("p1".into())]
    );
}

#[tokio::test]
async fn a_start_that_fails_after_its_project_was_removed_leaves_no_cache() {
    let test = setup(FakeLanguageServers::default(), true).await;
    let gate = test.servers.hold_starts();
    let core = test.core.clone();
    // No fake server for python: the start fails once let through.
    let starting = tokio::spawn(async move { start(&core, "python").await });
    while test.core.lsp_servers().lock().servers.is_empty() {
        tokio::task::yield_now().await;
    }
    let remove = json!({"method": "projects.remove", "params": {"id": "p1"}});
    ok(call(&test.core, remove).await);
    gate.add_permits(1);
    let started = starting.await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(code(started), ErrorCode::NotFound);
    let p1 = ProjectId("p1".into());
    assert_eq!(test.servers.forgotten(), [p1.clone(), p1]);
}

#[test]
fn server_uris_that_could_leave_the_project_never_reach_the_client_as_files() {
    let root = Path::new(ROOT);
    let file = |path: &str| format!("{}/{path}", uris::file_uri(root));
    for path in ["../../etc/x", "src/./a.rs", "a//b.rs", "a\\b.rs"] {
        let mut message = json!({"uri": file(path)});
        uris::to_client(&mut message, root);
        assert_eq!(message["uri"], file(path), "{path}");
    }
    for (sent, seen) in [
        ("ovra:///src/a%20b.rs", "ovra:///src/a%20b.rs"),
        ("ovra:///", "ovra:///"),
        ("ovra:///src/", "ovra:///src/"),
        ("ovra:///../etc/passwd", uris::INVALID),
        ("ovra:///src/%2E%2E/%2E%2E/x", uris::INVALID),
        ("ovra:///a%2F..%2F..%2Fx", uris::INVALID),
        ("ovra:///a%5Cb", uris::INVALID),
        ("ovra:///./a", uris::INVALID),
        ("ovra:////etc/passwd", uris::INVALID),
        ("ovra:///a?x=1", uris::INVALID),
        ("ovra:///%ZZ", uris::INVALID),
        ("ovra://host/a", uris::INVALID),
        ("OVRA:///a", uris::INVALID),
    ] {
        let mut message = json!({"uri": sent, "changes": {sent: []}});
        uris::to_client(&mut message, root);
        assert_eq!(message["uri"], seen, "{sent}");
        assert!(message["changes"].get(seen).is_some(), "{sent}");
    }
}

#[test]
fn uris_round_trip_on_this_os() {
    let root = Path::new(ROOT);
    let file = uris::file_uri(&root.join("src").join("a b#c.rs"));
    let mut message = json!({"uri": file});
    uris::to_client(&mut message, root);
    assert_eq!(message["uri"], "ovra:///src/a%20b%23c.rs");
    ok(uris::to_server(&mut message, root));
    assert_eq!(message["uri"], file);
    // Another host is another machine.
    let mut remote = json!({"uri": "file://server/work/a/x.rs"});
    uris::to_client(&mut remote, root);
    assert_eq!(remote["uri"], "file://server/work/a/x.rs");
    // A sibling folder that shares the root's name as a prefix is outside.
    let mut sibling = json!({"uri": uris::file_uri(Path::new(crate::test_path!("/work/ab/x.rs")))});
    let before = sibling.clone();
    uris::to_client(&mut sibling, root);
    assert_eq!(sibling, before);
}
