//! `lsp.*`: language servers for trusted projects (ADR 0007,
//! `docs/specs/lsp.md`). The core is the filter between the UI's LSP
//! client and each server: it starts servers only for trusted projects,
//! maps the UI's `ovra:///` URIs to files under the root and back, and
//! answers the requests a server makes of the client. The process is the
//! [`LanguageServers`](crate::services::LanguageServers) service's.

mod answer;
mod uris;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use crate::Core;
use crate::core::WeakCore;
use crate::protocol::lsp::{
    LspEvent, LspRequest, LspResponse, LspStatus, ServerId, ServerInfo, ServerState,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{RunningServer, ServerSpec};

/// Servers running at once, over all projects.
const MAX_RUNNING: usize = 4;

/// Events not yet received by every client past which a server's output
/// is held back, as terminal output is.
const MAX_BACKLOG: usize = 256;

/// Bytes of server messages possibly still in the event channel past which
/// a server's output is held back too. A message's length is its JSON's;
/// as a `serde_json::Value` it can take ~16 times that.
const MAX_IN_FLIGHT: usize = 16 << 20;

/// The longest language id a client may name.
const MAX_LANGUAGE: usize = 40;

/// The servers the core started, by id.
#[derive(Debug, Default)]
pub(crate) struct Servers {
    state: Mutex<State>,
    /// The lengths of the last [`MAX_BACKLOG`] server messages emitted, of
    /// every server, last last. The events in the channel are the last
    /// `backlog` ones, so the server messages among them are at most the
    /// last `backlog` of these.
    emitted: Mutex<VecDeque<usize>>,
}

#[derive(Debug, Default)]
struct State {
    started: u64,
    servers: BTreeMap<ServerId, Entry>,
}

#[derive(Debug)]
struct Entry {
    info: ServerInfo,
    root: PathBuf,
    /// `None` while the service starts it.
    process: Option<Arc<dyn RunningServer>>,
}

impl Servers {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn emitted(&self) -> std::sync::MutexGuard<'_, VecDeque<usize>> {
        self.emitted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether server messages fill the event channel: too many events,
    /// or too many bytes in the last `backlog` server messages.
    fn backed_up(&self, backlog: usize) -> bool {
        backlog >= MAX_BACKLOG
            || self.emitted().iter().rev().take(backlog).sum::<usize>() >= MAX_IN_FLIGHT
    }
}

pub(crate) async fn handle(core: &Core, request: LspRequest) -> Result<Response, ProtocolError> {
    match request {
        LspRequest::Status { project } => {
            let trusted = core.project_store().get(&project)?.trusted;
            let service = core.language_servers();
            let servers = core
                .lsp_servers()
                .lock()
                .servers
                .values()
                .filter(|e| e.info.project == project)
                .map(|e| e.info.clone())
                .collect();
            Ok(Response::Lsp(LspResponse::Status(LspStatus {
                trusted,
                sandbox: service.sandbox(),
                servers,
                known: service.known().await,
            })))
        }
        LspRequest::Start { project, language } => start(core, project, language)
            .await
            .map(|info| Response::Lsp(LspResponse::Server(info))),
        LspRequest::Send { server, message } => {
            send(core, &server, message)?;
            Ok(Response::Ok(OkResponse::Ok))
        }
        LspRequest::Stop { server } => {
            if !stop(core, &server) {
                return Err(gone(&server));
            }
            Ok(Response::Ok(OkResponse::Ok))
        }
    }
}

/// Stops every server of `project` and has the service forget its caches:
/// its trust was taken back, or it was removed.
pub(crate) fn stop_project(core: &Core, project: &ProjectId) {
    let ids: Vec<ServerId> = core
        .lsp_servers()
        .lock()
        .servers
        .values()
        .filter(|e| &e.info.project == project)
        .map(|e| e.info.id.clone())
        .collect();
    for id in ids {
        stop(core, &id);
    }
    core.language_servers().forget(project);
}

async fn start(
    core: &Core,
    project: ProjectId,
    language: String,
) -> Result<ServerInfo, ProtocolError> {
    let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-_.".contains(c);
    if language.is_empty() || language.len() > MAX_LANGUAGE || !language.chars().all(valid) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "not a language id",
        ));
    }
    let found = core.project_store().get(&project)?;
    if !found.trusted {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "language servers run the project's code: trust the project first",
        ));
    }
    let root = PathBuf::from(&found.root);
    let id = {
        let mut state = core.lsp_servers().lock();
        let same = state
            .servers
            .values()
            .find(|e| e.info.project == project && e.info.language == language);
        if let Some(entry) = same {
            return Ok(entry.info.clone());
        }
        if state.servers.len() >= MAX_RUNNING {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                format!("{MAX_RUNNING} language servers already run; stop one first"),
            ));
        }
        state.started += 1;
        let id = ServerId(format!("l{}", state.started));
        let info = ServerInfo {
            id: id.clone(),
            project: project.clone(),
            language: language.clone(),
            name: language.clone(),
            state: ServerState::Starting,
            sandbox: None,
        };
        state.servers.insert(
            id.clone(),
            Entry {
                info,
                root: root.clone(),
                process: None,
            },
        );
        id
    };
    let host = ServerHost {
        core: core.downgrade(),
        server: id.clone(),
        root: root.clone(),
        root_uri: uris::file_uri(&root),
        name: found.name,
        #[cfg(any(test, feature = "fakes"))]
        record: None,
    };
    let spec = ServerSpec {
        project: project.clone(),
        root,
        language,
    };
    let started = core.language_servers().start(spec, host).await;
    let mut state = core.lsp_servers().lock();
    let started = match started {
        Ok(started) => started,
        Err(e) => {
            let entry = state.servers.remove(&id);
            drop(state);
            // The service may have made the cache folder before it failed.
            forget_if_gone(core, &project);
            if let Some(entry) = entry {
                let mut info = entry.info;
                info.state = ServerState::Failed {
                    reason: e.message.clone(),
                };
                core.emit(LspEvent::Changed { server: info });
            }
            return Err(e);
        }
    };
    let process: Arc<dyn RunningServer> = Arc::from(started.process);
    let Some(entry) = state.servers.get_mut(&id) else {
        // It ended, or was stopped, before it was ready.
        drop(state);
        process.stop();
        forget_if_gone(core, &project);
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "the language server ended as it started",
        ));
    };
    entry.process = Some(process);
    entry.info.name = started.name;
    entry.info.sandbox = started.sandbox;
    entry.info.state = ServerState::Running;
    let info = entry.info.clone();
    drop(state);
    core.emit(LspEvent::Changed {
        server: info.clone(),
    });
    Ok(info)
}

/// Forgets `project`'s caches again if it was removed or lost its trust
/// while a server started: the service may have made the server's cache
/// folder after the core had it forget the project's.
fn forget_if_gone(core: &Core, project: &ProjectId) {
    if !core.project_store().get(project).is_ok_and(|p| p.trusted) {
        core.language_servers().forget(project);
    }
}

fn send(core: &Core, server: &ServerId, mut message: Value) -> Result<(), ProtocolError> {
    let (process, root) = {
        let state = core.lsp_servers().lock();
        let entry = state.servers.get(server).ok_or_else(|| gone(server))?;
        let process = entry.process.clone().ok_or_else(|| {
            ProtocolError::new(ErrorCode::Conflict, "the language server is still starting")
        })?;
        (process, entry.root.clone())
    };
    let Some(object) = message.as_object_mut() else {
        return Err(bad("a JSON-RPC message is an object"));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(bad("not a JSON-RPC 2.0 message"));
    }
    // v1 passes no server request on, so the client has nothing to answer.
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Err(bad("the client sends requests and notifications only"));
    };
    if method == "initialize"
        && let Some(params) = object.get_mut("params").and_then(Value::as_object_mut)
    {
        // The UI's process id means nothing to the server.
        params.insert("processId".into(), Value::Null);
    }
    uris::to_server(&mut message, &root)?;
    process.send(message)
}

/// Stops `server` if it is there; whether it was.
fn stop(core: &Core, server: &ServerId) -> bool {
    let Some(entry) = core.lsp_servers().lock().servers.remove(server) else {
        return false;
    };
    let mut info = entry.info;
    info.state = ServerState::Stopped;
    core.emit(LspEvent::Changed { server: info });
    if let Some(process) = entry.process {
        let exit = serde_json::json!({ "jsonrpc": "2.0", "method": "exit" });
        let _ = process.send(exit);
        process.stop();
    }
    true
}

fn gone(server: &ServerId) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        format!("language server {} not found", server.0),
    )
}

fn bad(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// What a [`LanguageServers`](crate::services::LanguageServers) service
/// hands one server's output to.
#[derive(Debug, Clone)]
pub struct ServerHost {
    core: WeakCore,
    server: ServerId,
    root: PathBuf,
    root_uri: String,
    name: String,
    /// What the host was handed, for a service's own tests.
    #[cfg(any(test, feature = "fakes"))]
    record: Option<Arc<crate::fakes::HostRecord>>,
}

impl ServerHost {
    /// One message the server wrote, `size` bytes long. A request of the
    /// client is answered here; anything else goes to the UI with its URIs
    /// rewritten.
    pub fn message(&self, mut message: Value, size: usize) {
        #[cfg(any(test, feature = "fakes"))]
        if let Some(record) = &self.record {
            return record.message(message, size);
        }
        let Some(core) = self.core.upgrade() else {
            return;
        };
        if !message.is_object() {
            return;
        }
        let is_request = message.get("method").is_some() && message.get("id").is_some();
        if is_request {
            let process = core
                .lsp_servers()
                .lock()
                .servers
                .get(&self.server)
                .and_then(|e| e.process.clone());
            if let Some(process) = process {
                let _ = process.send(answer::reply(&message, &self.root_uri, &self.name));
            }
            return;
        }
        uris::to_client(&mut message, &self.root);
        // Held while emitting, so the lengths keep the channel's order.
        let mut emitted = core.lsp_servers().emitted();
        core.emit(LspEvent::Message {
            server: self.server.clone(),
            message,
        });
        emitted.push_back(size);
        if emitted.len() > MAX_BACKLOG {
            emitted.pop_front();
        }
    }

    /// The server ended. `reason` says why if the service ended it; `code`
    /// is missing when a signal killed it.
    pub fn exited(&self, code: Option<i32>, reason: Option<String>) {
        #[cfg(any(test, feature = "fakes"))]
        if let Some(record) = &self.record {
            return record.exited(code, reason);
        }
        let Some(core) = self.core.upgrade() else {
            return;
        };
        let Some(entry) = core.lsp_servers().lock().servers.remove(&self.server) else {
            // The core stopped it and said so.
            return;
        };
        let mut info = entry.info;
        info.state = match (code, reason) {
            (_, Some(reason)) => ServerState::Failed { reason },
            (Some(0), None) => ServerState::Stopped,
            (Some(code), None) => ServerState::Failed {
                reason: format!("the server exited with code {code}"),
            },
            (None, None) => ServerState::Failed {
                reason: "the server was killed".into(),
            },
        };
        core.emit(LspEvent::Changed { server: info });
    }

    /// Whether the clients are behind; the service holds output back then.
    pub fn busy(&self) -> bool {
        #[cfg(any(test, feature = "fakes"))]
        if let Some(record) = &self.record {
            return record.busy();
        }
        self.core.upgrade().is_some_and(|core| {
            let backlog = core.sink().backlog();
            core.lsp_servers().backed_up(backlog)
        })
    }
}

#[cfg(any(test, feature = "fakes"))]
impl ServerHost {
    /// A host for a service's own tests: it belongs to no core and keeps
    /// what it is handed in the returned record.
    pub fn recording() -> (Self, Arc<crate::fakes::HostRecord>) {
        let record = Arc::new(crate::fakes::HostRecord::default());
        let (events, _) = tokio::sync::broadcast::channel(1);
        let host = Self {
            core: WeakCore::dangling(events),
            server: ServerId("l1".into()),
            root: PathBuf::new(),
            root_uri: String::new(),
            name: String::new(),
            record: Some(record.clone()),
        };
        (host, record)
    }
}

#[cfg(test)]
mod tests;
