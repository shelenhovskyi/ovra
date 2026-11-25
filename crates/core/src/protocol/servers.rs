//! `servers.*`: a project's dev servers, run in sandbox pods
//! (`docs/specs/servers-and-browser.md`, ADR 0008).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::projects::ProjectId;

/// Assigned by the core when a server starts; kept across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(pub String);

/// One server as the person defines it in `.ovra/servers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerDef {
    /// `^[a-z0-9][a-z0-9-]{0,31}$`, unique in the file.
    pub name: String,
    /// A program and its arguments, run without a shell.
    pub command: Vec<String>,
    /// Relative to the work dir; `None` is the work dir itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// What it listens on in its pod, 1024 to 65535.
    pub port: u16,
    /// `None`: ready when the port accepts a connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<ServerReady>,
}

/// When a server counts as ready; with both set, both must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerReady {
    /// A substring of a line of its output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// A path that must answer over HTTP, through the pod's bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerStatus {
    Starting,
    Ready,
    /// A stop waits for the process to end.
    Stopping,
    /// `code` is missing when it was killed by a signal.
    Exited {
        code: Option<i32>,
    },
    /// It could not start, or never became ready.
    Failed {
        reason: String,
    },
}

/// Who started a server. Neither owns it: it runs until stopped or Ovra quits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartedBy {
    Person,
    Agent { session: SessionId },
}

/// A started server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub id: ServerId,
    pub project: ProjectId,
    pub name: String,
    /// The session whose worktree it runs in; `None` for the main checkout.
    pub work_dir: Option<SessionId>,
    pub status: ServerStatus,
    /// Counts the starts of this server; each start gets a new one.
    pub generation: u64,
    pub port: u16,
    pub started_by: StartedBy,
}

/// The definitions and started servers of one work dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerList {
    pub definitions: Vec<ServerDef>,
    pub servers: Vec<ServerInfo>,
    /// Why `.ovra/servers.json` could not be read; `definitions` is then empty.
    pub error: Option<String>,
}

/// One line of a server's output, numbered from 1 per server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub n: u64,
    /// Without the line ending; ANSI escapes kept for the UI.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    /// The `since` that continues after this page.
    pub next: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ServersRequest {
    /// Answers `servers.list`.
    #[serde(rename = "servers.list")]
    List {
        project: ProjectId,
        #[serde(default)]
        work_dir: Option<SessionId>,
    },
    /// Starts a defined server. Answers `servers.server`.
    #[serde(rename = "servers.start")]
    Start {
        project: ProjectId,
        name: String,
        #[serde(default)]
        work_dir: Option<SessionId>,
    },
    /// SIGTERM, then SIGKILL after `grace_ms` (default 5000). Answers `ok`.
    #[serde(rename = "servers.stop")]
    Stop {
        id: ServerId,
        #[serde(default)]
        grace_ms: Option<u64>,
    },
    /// Stop, then start with a new generation. Answers `servers.server`.
    #[serde(rename = "servers.restart")]
    Restart { id: ServerId },
    /// Output lines numbered above `since`, `grep` a substring. Answers
    /// `servers.logs`.
    #[serde(rename = "servers.logs")]
    Logs {
        id: ServerId,
        #[serde(default)]
        since: Option<u64>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        grep: Option<String>,
    },
    /// Makes a ready server reachable on the host's loopback for the
    /// person's browser. Answers `servers.opened`.
    #[serde(rename = "servers.open")]
    Open { id: ServerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServersResponse {
    #[serde(rename = "servers.list")]
    List(ServerList),
    #[serde(rename = "servers.server")]
    Server(ServerInfo),
    #[serde(rename = "servers.logs")]
    Logs(LogPage),
    #[serde(rename = "servers.opened")]
    Opened { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServersEvent {
    #[serde(rename = "servers.status")]
    Status {
        id: ServerId,
        project: ProjectId,
        status: ServerStatus,
        generation: u64,
    },
    /// New output, batched and rate-limited, for the UI.
    #[serde(rename = "servers.output")]
    Output { id: ServerId, lines: Vec<LogLine> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::audit::AuditEvent;
    use crate::protocol::{Event, Request, Response};

    fn id() -> ServerId {
        ServerId("srv1".into())
    }

    #[test]
    fn wire_format_is_stable() {
        let def = ServerDef {
            name: "web".into(),
            command: vec!["pnpm".into(), "dev".into()],
            cwd: Some("ui".into()),
            env: BTreeMap::from([("VITE_API".into(), "http://localhost:8080".into())]),
            port: 5173,
            ready: Some(ServerReady {
                log: Some("ready in".into()),
                http: None,
            }),
        };
        let info = ServerInfo {
            id: id(),
            project: ProjectId("p1".into()),
            name: "web".into(),
            work_dir: Some(SessionId("s1".into())),
            status: ServerStatus::Ready,
            generation: 2,
            port: 5173,
            started_by: StartedBy::Agent {
                session: SessionId("s1".into()),
            },
        };
        let line = LogLine {
            n: 7,
            text: "ready in 300 ms".into(),
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Servers(ServersRequest::List { project: ProjectId("p1".into()), work_dir: None }),
                Request::Servers(ServersRequest::Stop { id: id(), grace_ms: Some(1000) }),
                Request::Servers(ServersRequest::Logs { id: id(), since: Some(6), limit: Some(100), grep: Some("error".into()) }),
            ],
            "responses": [
                Response::Servers(ServersResponse::List(ServerList {
                    definitions: vec![def, ServerDef {
                        name: "api".into(),
                        command: vec!["cargo".into(), "run".into()],
                        cwd: None,
                        env: BTreeMap::new(),
                        port: 8080,
                        ready: None,
                    }],
                    servers: vec![info.clone()],
                    error: None,
                })),
                Response::Servers(ServersResponse::Server(ServerInfo {
                    status: ServerStatus::Failed { reason: "port 5173 is taken".into() },
                    started_by: StartedBy::Person,
                    work_dir: None,
                    ..info
                })),
                Response::Servers(ServersResponse::Logs(LogPage { lines: vec![line.clone()], next: 7 })),
                Response::Servers(ServersResponse::Opened { url: "http://127.0.0.1:5173/".into() }),
            ],
            "events": [
                Event::from(ServersEvent::Status {
                    id: id(),
                    project: ProjectId("p1".into()),
                    status: ServerStatus::Exited { code: Some(1) },
                    generation: 2,
                }),
                Event::from(ServersEvent::Output { id: id(), lines: vec![line] }),
            ],
            "audit": [
                AuditEvent::ServerStarted { server: id(), name: "web".into(), generation: 2, by: StartedBy::Person },
                AuditEvent::ServerStopped { server: id(), name: "web".into(), generation: 2, code: None },
                AuditEvent::LoopbackGranted { server: id(), port: 5173, generation: 2 },
                AuditEvent::LoopbackRevoked { server: id(), port: 5173, generation: 2 },
            ],
        }));
    }

    #[test]
    fn every_request_round_trips() {
        let requests = [
            serde_json::json!({"method": "servers.list", "params": {"project": "p1", "work_dir": "s1"}}),
            serde_json::json!({"method": "servers.start", "params": {"project": "p1", "name": "web", "work_dir": null}}),
            serde_json::json!({"method": "servers.stop", "params": {"id": "srv1", "grace_ms": null}}),
            serde_json::json!({"method": "servers.restart", "params": {"id": "srv1"}}),
            serde_json::json!({"method": "servers.logs", "params": {"id": "srv1", "since": null, "limit": 10, "grep": null}}),
            serde_json::json!({"method": "servers.open", "params": {"id": "srv1"}}),
        ];
        for json in requests {
            let parsed: Request = serde_json::from_value(json.clone()).expect("request");
            assert!(matches!(parsed, Request::Servers(_)), "{json}");
            assert_eq!(serde_json::to_value(&parsed).ok(), Some(json));
        }
        // Optional parameters may be left out.
        let short: Request = serde_json::from_value(
            serde_json::json!({"method": "servers.list", "params": {"project": "p1"}}),
        )
        .expect("short");
        assert!(matches!(
            short,
            Request::Servers(ServersRequest::List { work_dir: None, .. })
        ));
    }
}
