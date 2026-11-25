//! Managed dev servers (`docs/specs/servers-and-browser.md`, ADR 0008):
//! definitions, states, generations and logs, the pods' netproxies and the
//! loopback grants of the sessions that may reach them. Processes and pods
//! are the [`DevServers`] service's.
//!
//! Lock order: this state's lock may be held while taking the sessions'
//! lock or calling the netproxy, never the other way round. Nothing here
//! calls [`DevServers`] with the lock held: it may report at once.

mod defs;
mod grants;
mod logs;
mod pods;
mod tools;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::Core;
use crate::core::WeakCore;
use crate::protocol::agents::{SessionId, SessionMode};
use crate::protocol::audit::AuditEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::{
    LogLine, ServerId, ServerInfo, ServerList, ServerStatus, ServersEvent, ServersRequest,
    ServersResponse, StartedBy,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{DevServerSpec, DevServers, NetProxy, SandboxStatus, ServerSink};

pub(crate) use grants::{forward, session_ended, session_started};
pub(crate) use pods::allowlist_changed;

type Result<T> = std::result::Result<T, ProtocolError>;

/// How long a stop waits before SIGKILL, unless the request says.
const DEFAULT_GRACE: Duration = Duration::from_secs(5);
/// How much longer than its grace a restart waits for the old process.
const STOP_SLACK: Duration = Duration::from_secs(5);
/// At most one `servers.output` per server in this time.
const OUTPUT_INTERVAL: Duration = Duration::from_millis(100);
/// Lines one `servers.output` holds; the UI pages older ones from the log.
const MAX_OUTPUT_LINES: usize = 500;

#[derive(Debug)]
pub(crate) struct DevServerState {
    dev: Arc<dyn DevServers>,
    net: Option<Arc<dyn NetProxy>>,
    state: Mutex<State>,
    /// Held by each start, so a pod's netproxy starts once.
    starting: tokio::sync::Mutex<()>,
    /// Bumped on every status change, for those who wait for one.
    changed: watch::Sender<u64>,
}

#[derive(Debug, Default)]
struct State {
    servers: Vec<Entry>,
    made: u64,
    pods: BTreeMap<PathBuf, pods::Pod>,
    pods_made: u64,
    /// What each session's netproxy was granted last.
    granted: BTreeMap<SessionId, Vec<grants::Granted>>,
}

/// The main checkout or a worker's worktree: one pod each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    project: ProjectId,
    /// The worker whose worktree it is; `None` for the main checkout.
    session: Option<SessionId>,
    dir: PathBuf,
}

/// A server started at least once; its id, log and generations stay
/// across restarts.
#[derive(Debug)]
struct Entry {
    id: ServerId,
    name: String,
    place: Place,
    status: ServerStatus,
    generation: u64,
    port: u16,
    started_by: StartedBy,
    log: logs::Log,
    /// Output not yet sent as `servers.output`.
    pending: Vec<LogLine>,
    last_output: Option<Instant>,
    flush_scheduled: bool,
}

impl Entry {
    fn info(&self) -> ServerInfo {
        ServerInfo {
            id: self.id.clone(),
            project: self.place.project.clone(),
            name: self.name.clone(),
            work_dir: self.place.session.clone(),
            status: self.status.clone(),
            generation: self.generation,
            port: self.port,
            started_by: self.started_by.clone(),
        }
    }

    fn running(&self) -> bool {
        !ended(&self.status)
    }
}

fn ended(status: &ServerStatus) -> bool {
    matches!(
        status,
        ServerStatus::Exited { .. } | ServerStatus::Failed { .. }
    )
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl DevServerState {
    pub(crate) fn new(dev: Arc<dyn DevServers>, net: Option<Arc<dyn NetProxy>>) -> Self {
        Self {
            dev,
            net,
            state: Mutex::new(State::default()),
            starting: tokio::sync::Mutex::new(()),
            changed: watch::Sender::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

pub(crate) async fn handle(core: &Core, request: ServersRequest) -> Result<Response> {
    let answer = match request {
        ServersRequest::List { project, work_dir } => {
            let place = place(core, &project, work_dir.as_ref())?;
            ServersResponse::List(list(core, &place).await)
        }
        ServersRequest::Start {
            project,
            name,
            work_dir,
        } => {
            let place = place(core, &project, work_dir.as_ref())?;
            ServersResponse::Server(start(core, place, &name, StartedBy::Person).await?)
        }
        ServersRequest::Stop { id, grace_ms } => {
            let grace = grace_ms.map_or(DEFAULT_GRACE, Duration::from_millis);
            stop(core, &id, grace)?;
            return Ok(Response::Ok(OkResponse::Ok));
        }
        ServersRequest::Restart { id } => {
            ServersResponse::Server(restart(core, &id, StartedBy::Person).await?)
        }
        ServersRequest::Logs {
            id,
            since,
            limit,
            grep,
        } => {
            let servers = core.dev_servers();
            let state = servers.lock();
            let entry = find(&state, &id)?;
            let page = entry
                .log
                .page(since.unwrap_or(0), limit, grep.as_deref(), false);
            ServersResponse::Logs(page)
        }
        // A follow-up: the person's forward on the host.
        ServersRequest::Open { .. } => {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "opening servers in a browser is not available in this build",
            ));
        }
    };
    Ok(Response::Servers(answer))
}

/// The work dir a request names: a session's (a worker's worktree, or the
/// main checkout for any other session) or the main checkout.
pub(crate) fn place(
    core: &Core,
    project: &ProjectId,
    session: Option<&SessionId>,
) -> Result<Place> {
    let root = core.root(project)?;
    let main = Place {
        project: project.clone(),
        session: None,
        dir: root,
    };
    let Some(session) = session else {
        return Ok(main);
    };
    let (info, launch) = core.sessions().find(session, |s| (s.info(), s.launch()))?;
    if &info.project != project {
        return Err(not_found(format!("session {}", session.0)));
    }
    if info.mode != SessionMode::Isolated {
        return Ok(main);
    }
    let launch = launch
        .ok_or_else(|| conflict("this worker's worktree is not known since Ovra restarted"))?;
    Ok(Place {
        session: Some(session.clone()),
        dir: launch.cwd,
        ..main
    })
}

async fn list(core: &Core, place: &Place) -> ServerList {
    let (definitions, error) = match defs::read(core.fs_service(), &place.dir).await {
        Ok(defs) => (defs, None),
        Err(why) => (Vec::new(), Some(why)),
    };
    let servers = core
        .dev_servers()
        .lock()
        .servers
        .iter()
        .filter(|e| e.place == *place)
        .map(Entry::info)
        .collect();
    ServerList {
        definitions,
        servers,
        error,
    }
}

/// Starts the defined server `name` in `place`, which must not run.
async fn start(core: &Core, place: Place, name: &str, by: StartedBy) -> Result<ServerInfo> {
    let servers = core.dev_servers();
    if let SandboxStatus::Unavailable { reason } = servers.dev.status() {
        return Err(ProtocolError::new(ErrorCode::Unavailable, reason));
    }
    let _one = servers.starting.lock().await;
    let defs = defs::read(core.fs_service(), &place.dir)
        .await
        .map_err(|why| ProtocolError::new(ErrorCode::BadRequest, why))?;
    let def = defs
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| not_found(format!("server {name} in {}", defs::FILE)))?;
    // The pod's last server may end while its netproxy starts.
    let mut tries = 0;
    let (pod, spec) = loop {
        let pod = pods::pod(core, &place, &defs).await?;
        match register(core, &place, def, &by) {
            Ok(Some(spec)) => break (pod, spec),
            Ok(None) => {}
            Err(err) => {
                pods::end_pod_if_idle(servers, &mut servers.lock(), &place);
                return Err(err);
            }
        }
        tries += 1;
        if tries == 3 {
            return Err(conflict("the server's pod kept ending while it started"));
        }
    };
    let sink: Arc<dyn ServerSink> = Arc::new(Sink(core.downgrade()));
    let (id, generation) = (spec.id.clone(), spec.generation);
    match servers.dev.start(&pod, spec, sink) {
        Ok(()) => {
            let event = AuditEvent::ServerStarted {
                server: id.clone(),
                name: name.to_owned(),
                generation,
                by,
            };
            crate::audit::record(core, &place.project, place.session.as_ref(), event);
        }
        Err(err) => {
            let failed = ServerStatus::Failed {
                reason: err.message,
            };
            report(core, &id, generation, failed, false);
        }
    }
    let state = servers.lock();
    find(&state, &id).map(Entry::info)
}

/// Makes the next generation of `def` in `place` `starting`; `None` when
/// the pod went away meanwhile.
fn register(
    core: &Core,
    place: &Place,
    def: &crate::protocol::servers::ServerDef,
    by: &StartedBy,
) -> Result<Option<DevServerSpec>> {
    let mut state = core.dev_servers().lock();
    if !state.pods.contains_key(&place.dir) {
        return Ok(None);
    }
    let at = state
        .servers
        .iter()
        .position(|e| e.place == *place && e.name == def.name);
    let at = match at {
        Some(at) if state.servers[at].running() => {
            return Err(conflict(format!("server {} already runs", def.name)));
        }
        Some(at) => at,
        None => {
            state.made += 1;
            let id = ServerId(format!("srv{}", state.made));
            state.servers.push(Entry {
                id,
                name: def.name.clone(),
                place: place.clone(),
                status: ServerStatus::Starting,
                generation: 0,
                port: def.port,
                started_by: by.clone(),
                log: logs::Log::default(),
                pending: Vec::new(),
                last_output: None,
                flush_scheduled: false,
            });
            state.servers.len() - 1
        }
    };
    let entry = &mut state.servers[at];
    entry.generation += 1;
    entry.status = ServerStatus::Starting;
    entry.port = def.port;
    entry.started_by = by.clone();
    let spec = DevServerSpec {
        id: entry.id.clone(),
        generation: entry.generation,
        line: defs::command_line(def, &place.dir),
        port: def.port,
        ready: def.ready.clone(),
    };
    emit_status(core, entry);
    Ok(Some(spec))
}

/// Asks a running server to end; its end is reported later.
fn stop(core: &Core, id: &ServerId, grace: Duration) -> Result<()> {
    let servers = core.dev_servers();
    {
        let mut state = servers.lock();
        let entry = find_mut(&mut state, id)?;
        if !matches!(entry.status, ServerStatus::Starting | ServerStatus::Ready) {
            return Ok(());
        }
        entry.status = ServerStatus::Stopping;
        emit_status(core, entry);
        let place = entry.place.clone();
        grants::regrant(core, &mut state, &place);
    }
    servers.changed.send_modify(|n| *n += 1);
    servers.dev.stop(id, grace);
    Ok(())
}

/// Stop, wait for the end, then start with a new generation.
async fn restart(core: &Core, id: &ServerId, by: StartedBy) -> Result<ServerInfo> {
    let (place, name) = {
        let state = core.dev_servers().lock();
        let entry = find(&state, id)?;
        (entry.place.clone(), entry.name.clone())
    };
    stop(core, id, DEFAULT_GRACE)?;
    let stopped = wait(core, id, DEFAULT_GRACE + STOP_SLACK, |e| !e.running()).await?;
    if stopped.status == ServerStatus::Stopping {
        return Err(conflict(format!("server {name} did not stop")));
    }
    start(core, place, &name, by).await
}

/// Waits until `done` holds for the server, or `limit` passed; returns
/// it as it is then.
async fn wait(
    core: &Core,
    id: &ServerId,
    limit: Duration,
    done: impl Fn(&Entry) -> bool,
) -> Result<ServerInfo> {
    let servers = core.dev_servers();
    let mut changed = servers.changed.subscribe();
    let check = || {
        let state = servers.lock();
        find(&state, id).map(|e| (done(e), e.info()))
    };
    let waiting = async {
        loop {
            let (done, info) = check()?;
            if done || changed.changed().await.is_err() {
                return Ok(info);
            }
        }
    };
    match tokio::time::timeout(limit, waiting).await {
        Ok(result) => result,
        Err(_) => check().map(|(_, info)| info),
    }
}

/// Reports to the core; holds it weakly, as the service outlives nothing.
#[derive(Debug)]
struct Sink(WeakCore);

impl ServerSink for Sink {
    fn output(&self, id: &ServerId, generation: u64, lines: Vec<String>) {
        if let Some(core) = self.0.upgrade() {
            output(&core, id, generation, lines);
        }
    }

    fn status(&self, id: &ServerId, generation: u64, status: ServerStatus) {
        if let Some(core) = self.0.upgrade() {
            report(&core, id, generation, status, true);
        }
    }
}

fn output(core: &Core, id: &ServerId, generation: u64, lines: Vec<String>) {
    let mut state = core.dev_servers().lock();
    let Some(entry) = state
        .servers
        .iter_mut()
        .find(|e| &e.id == id && e.generation == generation)
    else {
        return;
    };
    for text in lines {
        let line = entry.log.push(text);
        entry.pending.push(line);
    }
    let over = entry.pending.len().saturating_sub(MAX_OUTPUT_LINES);
    entry.pending.drain(..over);
    if entry.flush_scheduled {
        return;
    }
    let due = entry.last_output.map(|t| t + OUTPUT_INTERVAL);
    match (due, tokio::runtime::Handle::try_current()) {
        (Some(due), Ok(runtime)) if due > Instant::now() => {
            entry.flush_scheduled = true;
            let (core, id) = (core.downgrade(), id.clone());
            runtime.spawn(async move {
                tokio::time::sleep_until(due).await;
                if let Some(core) = core.upgrade() {
                    let mut state = core.dev_servers().lock();
                    if let Some(entry) = state.servers.iter_mut().find(|e| e.id == id) {
                        entry.flush_scheduled = false;
                        flush(&core, entry);
                    }
                }
            });
        }
        _ => flush(core, entry),
    }
}

/// Sends the pending output as one `servers.output`.
fn flush(core: &Core, entry: &mut Entry) {
    if entry.pending.is_empty() {
        return;
    }
    entry.last_output = Some(Instant::now());
    core.emit(ServersEvent::Output {
        id: entry.id.clone(),
        lines: std::mem::take(&mut entry.pending),
    });
}

/// A status the service reported for `generation`, or a failed start
/// (`started` false: no `ServerStopped`, as no `ServerStarted` came).
fn report(core: &Core, id: &ServerId, generation: u64, status: ServerStatus, started: bool) {
    let servers = core.dev_servers();
    {
        let mut state = servers.lock();
        let Some(entry) = state
            .servers
            .iter_mut()
            .find(|e| &e.id == id && e.generation == generation)
        else {
            return;
        };
        let allowed = match &status {
            ServerStatus::Ready => entry.status == ServerStatus::Starting,
            s if ended(s) => entry.running(),
            _ => false,
        };
        if !allowed {
            return;
        }
        flush(core, entry);
        entry.status = status.clone();
        emit_status(core, entry);
        let place = entry.place.clone();
        if started && ended(&status) {
            let code = match status {
                ServerStatus::Exited { code } => code,
                _ => None,
            };
            let event = AuditEvent::ServerStopped {
                server: id.clone(),
                name: entry.name.clone(),
                generation,
                code,
            };
            crate::audit::record(core, &place.project, place.session.as_ref(), event);
        }
        grants::regrant(core, &mut state, &place);
        if ended(&status) {
            pods::end_pod_if_idle(servers, &mut state, &place);
        }
    }
    servers.changed.send_modify(|n| *n += 1);
}

fn emit_status(core: &Core, entry: &Entry) {
    core.emit(ServersEvent::Status {
        id: entry.id.clone(),
        project: entry.place.project.clone(),
        status: entry.status.clone(),
        generation: entry.generation,
    });
}

fn find<'a>(state: &'a State, id: &ServerId) -> Result<&'a Entry> {
    state
        .servers
        .iter()
        .find(|e| &e.id == id)
        .ok_or_else(|| not_found(format!("server {}", id.0)))
}

fn find_mut<'a>(state: &'a mut State, id: &ServerId) -> Result<&'a mut Entry> {
    state
        .servers
        .iter_mut()
        .find(|e| &e.id == id)
        .ok_or_else(|| not_found(format!("server {}", id.0)))
}

fn not_found(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("{what} not found"))
}

fn conflict(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}

#[cfg(test)]
mod tests;
