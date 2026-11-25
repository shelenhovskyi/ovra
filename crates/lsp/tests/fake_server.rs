//! The real process path against the fake server binary (`fake-lsp`).

use std::path::Path;
use std::sync::Arc;

use ovra_core::fakes::{FakeSandbox, HostRecord};
use ovra_core::protocol::projects::ProjectId;
use ovra_core::services::{LanguageServers, SandboxStatus, ServerHost, ServerSpec};
use ovra_lsp::{ServerDef, Servers};
use serde_json::{Value, json};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-lsp");

fn servers(sandbox: FakeSandbox, caches: &Path) -> (Servers, Arc<FakeSandbox>) {
    let sandbox = Arc::new(sandbox);
    let def = ServerDef {
        language: "rust".into(),
        name: "fake-lsp".into(),
        program: FAKE.into(),
        args: vec![],
        install: "build it".into(),
        env: vec![],
    };
    let servers = Servers::with_known(sandbox.clone(), caches.into(), vec![def]);
    (servers, sandbox)
}

fn spec(root: &Path) -> ServerSpec {
    ServerSpec {
        project: ProjectId("p1".into()),
        root: root.into(),
        language: "rust".into(),
    }
}

fn unsandboxed() -> FakeSandbox {
    FakeSandbox {
        status: SandboxStatus::Unavailable {
            reason: "none here".into(),
        },
        ..FakeSandbox::available()
    }
}

fn with_id(record: &HostRecord, id: i64) -> Option<Value> {
    record.messages().into_iter().find(|m| m["id"] == id)
}

#[tokio::test]
async fn talks_to_the_server_and_reports_its_end_after_stop() {
    let dir = tempfile::tempdir().expect("tmp");
    let (servers, sandbox) = servers(unsandboxed(), &dir.path().join("lsp"));
    let (host, record) = ServerHost::recording();
    let started = servers.start(spec(dir.path()), host).await.expect("start");
    assert_eq!((started.name.as_str(), started.sandbox), ("fake-lsp", None));
    assert!(sandbox.servers.lock().expect("lock").is_empty());
    let server = started.process;
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
    server.send(initialize).expect("send");
    record.wait(|r| with_id(r, 1).is_some()).await;
    let messages = record.messages();
    assert_eq!(messages[0]["method"], "workspace/configuration");
    assert_eq!(messages[1]["result"]["capabilities"]["hoverProvider"], true);
    let text = "fn main() {}\n// TODO: more\n";
    server
        .send(json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {"uri": "file:///a.rs", "languageId": "rust", "version": 1, "text": text},
        }}))
        .expect("send");
    let hover = json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": {
        "textDocument": {"uri": "file:///a.rs"}, "position": {"line": 0, "character": 0},
    }});
    server.send(hover).expect("send");
    record.wait(|r| with_id(r, 2).is_some()).await;
    let messages = record.messages();
    let diagnostics = &messages[2]["params"]["diagnostics"];
    assert_eq!(messages[2]["method"], "textDocument/publishDiagnostics");
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 1, "character": 3})
    );
    assert_eq!(diagnostics[0]["message"], "fake: TODO found");
    assert_eq!(messages[3]["result"]["contents"]["value"], "fake hover");
    server.stop();
    record.wait(|r| r.exit().is_some()).await;
    assert_eq!(record.exit(), Some((Some(0), None)));
    assert!(server.send(json!({})).is_err());
}

#[tokio::test]
async fn runs_through_the_sandbox_and_reports_a_crash_with_stderr() {
    let dir = tempfile::tempdir().expect("tmp");
    let (servers, sandbox) = servers(FakeSandbox::available(), &dir.path().join("lsp"));
    let (host, record) = ServerHost::recording();
    let started = servers.start(spec(dir.path()), host).await.expect("start");
    assert_eq!(started.sandbox.as_deref(), Some("fake"));
    let profiles = sandbox.servers.lock().expect("lock").clone();
    assert_eq!(
        profiles[0].cache_dir,
        dir.path().join("lsp").join("p1").join("rust")
    );
    let crash = json!({"jsonrpc": "2.0", "id": 1, "method": "fake/crash"});
    started.process.send(crash).expect("send");
    record.wait(|r| r.exit().is_some()).await;
    let (code, reason) = record.exit().expect("exited");
    assert_eq!(code, Some(3));
    let reason = reason.expect("a reason");
    assert!(reason.ends_with(": fake: crashed on purpose"), "{reason}");
}

// `file://` plus a Unix path; Windows paths need more encoding than the fake does.
#[cfg(unix)]
#[tokio::test]
async fn the_fake_finds_and_renames_a_word_in_open_files_and_at_the_root() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("b.rs"), "use crate::greet;\n").expect("write");
    let (servers, _) = servers(unsandboxed(), &dir.path().join("lsp"));
    let (host, record) = ServerHost::recording();
    let server = servers
        .start(spec(dir.path()), host)
        .await
        .expect("start")
        .process;
    let root = format!("file://{}", dir.path().display());
    let a = format!("{root}/a.rs");
    let b = format!("{root}/b.rs");
    let send = |message: Value| server.send(message).expect("send");
    send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"rootUri": root}}));
    send(
        json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {"uri": a, "languageId": "rust", "version": 1, "text": "fn greet() {}\nfn greeting() { greet(); }\n"},
        }}),
    );
    let at = json!({"textDocument": {"uri": a}, "position": {"line": 0, "character": 4}});
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/references", "params": at}));
    let mut rename = at.clone();
    rename["newName"] = json!("hello");
    send(json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/rename", "params": rename}));
    record.wait(|r| with_id(r, 3).is_some()).await;
    let found = with_id(&record, 2).expect("references")["result"].clone();
    let places: Vec<(String, u64, u64)> = found
        .as_array()
        .expect("list")
        .iter()
        .map(|l| {
            let start = &l["range"]["start"];
            let uri = l["uri"].as_str().unwrap_or_default().to_owned();
            (
                uri,
                start["line"].as_u64().unwrap_or(9),
                start["character"].as_u64().unwrap_or(9),
            )
        })
        .collect();
    assert_eq!(
        places,
        [(a.clone(), 0, 3), (a.clone(), 1, 16), (b.clone(), 0, 11)]
    );
    let changes = &with_id(&record, 3).expect("rename")["result"]["changes"];
    assert_eq!(changes[&a].as_array().map(Vec::len), Some(2));
    assert_eq!(changes[&b][0]["newText"], "hello");
    server.stop();
}
