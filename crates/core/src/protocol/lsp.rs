//! `lsp.*`: language servers for the files of a trusted project (ADR 0007,
//! `docs/specs/lsp.md`). The UI's LSP client sends JSON-RPC messages with
//! `lsp.send` and receives the server's with `lsp.message`; the core
//! filters both ways.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// Assigned by the core when a server starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerState {
    Starting,
    Running,
    Stopped,
    Failed { reason: String },
}

/// One language server of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub id: ServerId,
    pub project: ProjectId,
    /// The editor's language id: `rust`, `typescript`, `python`, ...
    pub language: String,
    /// The server's name, such as `rust-analyzer`.
    pub name: String,
    #[serde(flatten)]
    pub state: ServerState,
    /// The sandbox it runs in; `None` when it runs unsandboxed.
    pub sandbox: Option<String>,
}

/// A server Ovra knows for a language, and whether it is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownServer {
    pub language: String,
    pub name: String,
    /// Where it was found; `None` when it is not installed.
    pub path: Option<String>,
    /// How to install it, for the person to run themselves.
    pub install: String,
}

/// What a project's language servers look like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspStatus {
    /// Whether servers may start: only for a trusted project.
    pub trusted: bool,
    /// The sandbox servers would run in; `None` for none.
    pub sandbox: Option<String>,
    pub servers: Vec<ServerInfo>,
    pub known: Vec<KnownServer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum LspRequest {
    /// Answers `lsp.status`.
    #[serde(rename = "lsp.status")]
    Status { project: ProjectId },
    /// Starts the server for `language`, or returns the one running.
    /// Answers `lsp.server`. `forbidden` for an untrusted project,
    /// `not_found` when no server is known or installed for the language.
    #[serde(rename = "lsp.start")]
    Start {
        project: ProjectId,
        language: String,
    },
    /// One JSON-RPC message for the server, with `ovra:///` URIs. Answers
    /// `ok`; `bad_request` for a message that names a `file:` URI or a
    /// path outside the project.
    #[serde(rename = "lsp.send")]
    Send {
        server: ServerId,
        message: serde_json::Value,
    },
    /// Answers `ok`; `lsp.changed` follows.
    #[serde(rename = "lsp.stop")]
    Stop { server: ServerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum LspResponse {
    #[serde(rename = "lsp.status")]
    Status(LspStatus),
    #[serde(rename = "lsp.server")]
    Server(ServerInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum LspEvent {
    /// One JSON-RPC message from the server, after the filter.
    #[serde(rename = "lsp.message")]
    Message {
        server: ServerId,
        message: serde_json::Value,
    },
    /// A server started, stopped or failed.
    #[serde(rename = "lsp.changed")]
    Changed { server: ServerInfo },
}
