use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use crate::agents::{self, Sessions};
use crate::orchestration::{self, Orchestration};
use crate::path::RelPath;
use crate::protocol::fs::{FsRequest, FsResponse, MAX_FILES_PAGE};
use crate::protocol::info::{CoreInfo, InfoEvent, InfoRequest, InfoResponse};
use crate::protocol::open::OpenRequest;
use crate::protocol::projects::{
    ProjectId, ProjectStatePatch, ProjectsEvent, ProjectsRequest, ProjectsResponse,
};
use crate::protocol::terminals::{TerminalsRequest, TerminalsResponse};
use crate::protocol::{ErrorCode, Event, OkResponse, ProtocolError, Request, Response};
use crate::services::{
    AgentRuntime, EventSink, FsService, IdeTools, Opener, ProjectStore, SessionStore,
    TerminalService, Unavailable, Worktrees,
};

const EVENT_CAPACITY: usize = 1024;

/// Wires services into a [`Core`]. Services left out answer `unavailable`.
#[derive(Debug)]
pub struct CoreBuilder {
    mode: String,
    projects: Arc<dyn ProjectStore>,
    fs: Arc<dyn FsService>,
    opener: Arc<dyn Opener>,
    terminals: Arc<dyn TerminalService>,
    agents: Arc<dyn AgentRuntime>,
    sessions: Arc<dyn SessionStore>,
    worktrees: Arc<dyn Worktrees>,
    worktrees_dir: Option<PathBuf>,
    ide_tools: Arc<dyn IdeTools>,
}

impl CoreBuilder {
    pub fn projects(mut self, store: Arc<dyn ProjectStore>) -> Self {
        self.projects = store;
        self
    }

    pub fn fs(mut self, fs: Arc<dyn FsService>) -> Self {
        self.fs = fs;
        self
    }

    pub fn opener(mut self, opener: Arc<dyn Opener>) -> Self {
        self.opener = opener;
        self
    }

    pub fn terminals(mut self, terminals: Arc<dyn TerminalService>) -> Self {
        self.terminals = terminals;
        self
    }

    pub fn agents(mut self, runtime: Arc<dyn AgentRuntime>) -> Self {
        self.agents = runtime;
        self
    }

    /// Where agent sessions are kept across restarts; without one they last
    /// for this run only.
    pub fn sessions(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.sessions = store;
        self
    }

    pub fn worktrees(mut self, worktrees: Arc<dyn Worktrees>) -> Self {
        self.worktrees = worktrees;
        self
    }

    /// Where workers' worktrees go: a folder in Ovra's data directory,
    /// outside every repository. Without one, coordinators cannot start
    /// workers.
    pub fn worktrees_dir(mut self, dir: PathBuf) -> Self {
        self.worktrees_dir = Some(dir);
        self
    }

    pub fn ide_tools(mut self, tools: Arc<dyn IdeTools>) -> Self {
        self.ide_tools = tools;
        self
    }

    pub fn build(self) -> Core {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let core = Core {
            inner: Arc::new(Inner {
                mode: self.mode,
                projects: self.projects,
                fs: self.fs,
                opener: self.opener,
                terminals: self.terminals,
                agents: self.agents,
                sessions: Arc::new(Sessions::new(self.sessions, self.worktrees.clone())),
                worktrees: self.worktrees,
                orchestration: Orchestration::new(self.worktrees_dir),
                held: Mutex::new(None),
                open: Mutex::new(BTreeMap::new()),
            }),
            events,
        };
        core.inner
            .sessions
            .link_ide(self.ide_tools, core.downgrade());
        core
    }
}

/// The Ovra core. Cheap to clone; clones share state.
#[derive(Debug, Clone)]
pub struct Core {
    inner: Arc<Inner>,
    events: broadcast::Sender<Event>,
}

#[derive(Debug)]
struct Inner {
    mode: String,
    projects: Arc<dyn ProjectStore>,
    fs: Arc<dyn FsService>,
    opener: Arc<dyn Opener>,
    terminals: Arc<dyn TerminalService>,
    agents: Arc<dyn AgentRuntime>,
    worktrees: Arc<dyn Worktrees>,
    orchestration: Orchestration,
    sessions: Arc<Sessions>,
    /// The project held by requests sent through [`Core::handle`] directly.
    held: Mutex<Option<ProjectId>>,
    /// Open projects and how many clients hold each; each is watched.
    open: Mutex<BTreeMap<ProjectId, usize>>,
}

/// A [`Core`] that does not keep it alive; sessions hold one so their IDE
/// tool calls can reach the core without a reference cycle.
#[derive(Debug, Clone)]
pub(crate) struct WeakCore {
    inner: std::sync::Weak<Inner>,
    events: broadcast::Sender<Event>,
}

impl WeakCore {
    pub(crate) fn upgrade(&self) -> Option<Core> {
        Some(Core {
            inner: self.inner.upgrade()?,
            events: self.events.clone(),
        })
    }
}

/// One connection's view of the core (a window or a browser tab). It holds
/// the project it opened last; the core watches a project while any client
/// holds it, and dropping the client lets go of it.
#[derive(Debug)]
pub struct Client {
    core: Core,
    held: Mutex<Option<ProjectId>>,
}

impl Client {
    /// Handles one request on behalf of this client.
    pub async fn handle(&self, request: Request) -> Result<Response, ProtocolError> {
        self.core.handle_for(request, &self.held).await
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let held = lock(&self.held).take();
        if let Some(id) = held {
            self.core.release(&id);
            self.core.emit_projects_changed();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Core {
    /// Starts building a core for a front-end mode (`tauri`, `headless`, ...).
    pub fn builder(mode: impl Into<String>) -> CoreBuilder {
        CoreBuilder {
            mode: mode.into(),
            projects: Arc::new(Unavailable("project store")),
            fs: Arc::new(Unavailable("file access")),
            opener: Arc::new(Unavailable("opening files externally")),
            terminals: Arc::new(Unavailable("terminals")),
            agents: Arc::new(Unavailable("agents")),
            sessions: Arc::new(Unavailable("keeping sessions")),
            worktrees: Arc::new(Unavailable("worktrees")),
            worktrees_dir: None,
            ide_tools: Arc::new(Unavailable("IDE tools")),
        }
    }

    /// A core with no services wired, e.g. for transport tests.
    pub fn new(mode: impl Into<String>) -> Self {
        Self::builder(mode).build()
    }

    /// Subscribes to events pushed by the core.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Waits for the next event for one subscriber. A subscriber that fell
    /// behind gets [`InfoEvent::Resync`] instead of the events it missed and
    /// keeps receiving; `None` means the core is gone.
    pub async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Option<Event> {
        match rx.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                Some(InfoEvent::Resync { skipped }.into())
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// A handle services use to push events.
    pub fn sink(&self) -> EventSink {
        EventSink(self.events.clone())
    }

    /// Pushes an event to every subscriber.
    pub fn emit(&self, event: impl Into<Event>) {
        self.sink().emit(event);
    }

    pub(crate) fn downgrade(&self) -> WeakCore {
        WeakCore {
            inner: Arc::downgrade(&self.inner),
            events: self.events.clone(),
        }
    }

    /// A client for one connection; see [`Client`].
    pub fn connect(&self) -> Client {
        Client {
            core: self.clone(),
            held: Mutex::new(None),
        }
    }

    /// Handles one request as a single implicit client (tests, one-window
    /// front ends).
    pub async fn handle(&self, request: Request) -> Result<Response, ProtocolError> {
        self.handle_for(request, &self.inner.held).await
    }

    async fn handle_for(
        &self,
        request: Request,
        held: &Mutex<Option<ProjectId>>,
    ) -> Result<Response, ProtocolError> {
        match request {
            Request::Info(InfoRequest::Get) => Ok(Response::Info(InfoResponse::Info(CoreInfo {
                name: "ovra".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                mode: self.inner.mode.clone(),
            }))),
            Request::Projects(r) => self.projects(r, held).await,
            Request::Fs(r) => self.fs(r).await.map(Response::Fs),
            Request::Open(r) => self.open(r).map(|()| Response::Ok(OkResponse::Ok)),
            Request::Terminals(r) => self.terminals(r),
            Request::Agents(r) => agents::handle_agents(&self.agents_ctx(), r).await,
            Request::Permissions(r) => agents::handle_permissions(&self.agents_ctx(), r),
            Request::Review(r) => orchestration::review(self, r).await,
            Request::Orchestration(r) => orchestration::orchestration(self, r).await,
        }
    }

    #[cfg(test)]
    pub(crate) fn inner_sessions(&self) -> Arc<Sessions> {
        self.inner.sessions.clone()
    }

    pub(crate) fn sessions(&self) -> &Arc<Sessions> {
        &self.inner.sessions
    }

    pub(crate) fn orchestration_state(&self) -> &Orchestration {
        &self.inner.orchestration
    }

    pub(crate) fn worktrees(&self) -> &dyn Worktrees {
        self.inner.worktrees.as_ref()
    }

    pub(crate) fn project_store(&self) -> &dyn ProjectStore {
        self.inner.projects.as_ref()
    }

    pub(crate) fn agents_ctx(&self) -> agents::Ctx<'_> {
        agents::Ctx {
            sessions: &self.inner.sessions,
            runtime: self.inner.agents.as_ref(),
            projects: self.inner.projects.as_ref(),
            sink: self.sink(),
        }
    }

    fn terminals(&self, request: TerminalsRequest) -> Result<Response, ProtocolError> {
        let terminals = &self.inner.terminals;
        match request {
            TerminalsRequest::Open { project, size } => {
                let id = terminals.open(self.root(&project)?, size, self.sink())?;
                return Ok(Response::Terminals(TerminalsResponse::Opened { id }));
            }
            TerminalsRequest::Input { id, data } => terminals.input(&id, &data)?,
            TerminalsRequest::Resize { id, size } => terminals.resize(&id, size)?,
            TerminalsRequest::Close { id } => terminals.close(&id)?,
        }
        Ok(Response::Ok(OkResponse::Ok))
    }

    /// Takes a reference on a project, watching it if it was not open.
    pub(crate) fn acquire(&self, id: &ProjectId, root: PathBuf) -> Result<(), ProtocolError> {
        let mut open = lock(&self.inner.open);
        if !open.contains_key(id) {
            self.inner.fs.watch(id.clone(), root, self.sink())?;
        }
        *open.entry(id.clone()).or_insert(0) += 1;
        Ok(())
    }

    /// Drops a reference; the last one stops watching.
    pub(crate) fn release(&self, id: &ProjectId) {
        let mut open = lock(&self.inner.open);
        if let Some(count) = open.get_mut(id) {
            *count -= 1;
            if *count == 0 {
                open.remove(id);
                self.inner.fs.unwatch(id);
            }
        }
    }

    async fn projects(
        &self,
        request: ProjectsRequest,
        held: &Mutex<Option<ProjectId>>,
    ) -> Result<Response, ProtocolError> {
        let store = &self.inner.projects;
        let response = match request {
            ProjectsRequest::List => store.list().map(ProjectsResponse::List),
            ProjectsRequest::Add { root } => {
                let root = PathBuf::from(root);
                if !root.is_absolute() {
                    return Err(ProtocolError::new(
                        ErrorCode::BadRequest,
                        "project root must be an absolute path",
                    ));
                }
                let project = store.add(&root)?;
                self.emit_projects_changed();
                Ok(ProjectsResponse::Project(project))
            }
            ProjectsRequest::Remove { id } => {
                store.remove(&id)?;
                agents::stop_project(&self.agents_ctx(), &id);
                // Clients still holding it let go of nothing when they close.
                if lock(&self.inner.open).remove(&id).is_some() {
                    self.inner.fs.unwatch(&id);
                }
                self.emit_projects_changed();
                Ok(ProjectsResponse::List(store.list()?))
            }
            ProjectsRequest::Open { id } => {
                let project = store.touch(&id)?;
                let mut held = lock(held);
                if held.as_ref() != Some(&id) {
                    self.acquire(&id, PathBuf::from(&project.root))?;
                    if let Some(previous) = held.replace(id) {
                        self.release(&previous);
                    }
                }
                drop(held);
                self.emit_projects_changed();
                Ok(ProjectsResponse::Project(project))
            }
            ProjectsRequest::Trust {
                id,
                trusted,
                fingerprint,
            } => {
                let project = agents::trust(&self.agents_ctx(), &id, trusted, fingerprint).await?;
                self.emit_projects_changed();
                Ok(ProjectsResponse::Project(project))
            }
            ProjectsRequest::GetState { id } => store.state(&id).map(ProjectsResponse::State),
            ProjectsRequest::SetState { id, state } => {
                store.update_state(&id, checked_patch(state)?)?;
                return Ok(Response::Ok(OkResponse::Ok));
            }
        };
        response.map(Response::Projects)
    }

    fn emit_projects_changed(&self) {
        let open = lock(&self.inner.open).keys().cloned().collect();
        self.emit(ProjectsEvent::Changed { open });
    }

    pub(crate) fn root(&self, project: &ProjectId) -> Result<PathBuf, ProtocolError> {
        Ok(PathBuf::from(self.inner.projects.get(project)?.root))
    }

    async fn fs(&self, request: FsRequest) -> Result<FsResponse, ProtocolError> {
        let fs = &self.inner.fs;
        match request {
            FsRequest::ListDir { project, path } => {
                let (root, path) = (self.root(&project)?, RelPath::parse(&path)?);
                fs.list_dir(root, path).await.map(FsResponse::Dir)
            }
            FsRequest::ReadFile { project, path } => {
                let (root, path) = (self.root(&project)?, RelPath::parse(&path)?);
                fs.read_file(root, path).await.map(FsResponse::File)
            }
            FsRequest::ListFiles {
                project,
                offset,
                limit,
            } => {
                let limit = limit.unwrap_or(MAX_FILES_PAGE).min(MAX_FILES_PAGE);
                fs.list_files(self.root(&project)?, offset, limit)
                    .await
                    .map(FsResponse::Files)
            }
            FsRequest::WriteFile {
                project,
                path,
                text,
                expected_mtime_ms,
            } => {
                let (root, path) = (self.root(&project)?, RelPath::parse(&path)?);
                if path.is_root() {
                    return Err(ProtocolError::new(
                        ErrorCode::BadRequest,
                        "cannot write the project root",
                    ));
                }
                fs.write_file(root, path, text, expected_mtime_ms)
                    .await
                    .map(|mtime_ms| FsResponse::Written { mtime_ms })
            }
        }
    }

    fn open(&self, request: OpenRequest) -> Result<(), ProtocolError> {
        match request {
            OpenRequest::External {
                project,
                path,
                line,
            } => {
                let file = RelPath::parse(&path)?.under(&self.root(&project)?);
                self.inner.opener.open_in_editor(&file, line)
            }
            OpenRequest::Url { url } => {
                check_web_url(&url)?;
                self.inner.opener.open_url(&url)
            }
        }
    }
}

/// Only web pages may leave the IDE: no `file:`, `javascript:` or custom
/// schemes that could start other programs, and nothing a system handler
/// could read as an option or a second argument.
fn check_web_url(url: &str) -> Result<(), ProtocolError> {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let has_host = ["http://", "https://"]
        .iter()
        .find_map(|scheme| lower.starts_with(scheme).then(|| &url[scheme.len()..]))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'));
    if has_host && !url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "only http:// and https:// URLs can be opened",
        ))
    }
}

/// Normalizes every path in `patch`, rejecting any that leaves the project.
fn checked_patch(patch: ProjectStatePatch) -> Result<ProjectStatePatch, ProtocolError> {
    let norm = |p: String| RelPath::parse(&p).map(|p| p.to_string());
    let all = |paths: Vec<String>| paths.into_iter().map(norm).collect::<Result<_, _>>();
    Ok(ProjectStatePatch {
        open_files: patch.open_files.map(all).transpose()?,
        active_file: patch
            .active_file
            .map(|file| file.map(norm).transpose())
            .transpose()?,
        expanded_dirs: patch.expanded_dirs.map(all).transpose()?,
    })
}

#[cfg(test)]
mod tests;
