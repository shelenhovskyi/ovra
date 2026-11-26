use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use crate::agents::{self, Sessions};
use crate::orchestration::{self, Orchestration};
use crate::path::RelPath;
use crate::protocol::audit::AuditEvent;
use crate::protocol::fs::{EntryKind, FsRequest, FsResponse, MAX_FILES_PAGE};
use crate::protocol::info::{CoreInfo, InfoEvent, InfoRequest, InfoResponse};
use crate::protocol::open::OpenRequest;
use crate::protocol::permissions::PermissionsRequest;
use crate::protocol::projects::{
    EditorGroup, ProjectId, ProjectStatePatch, ProjectsEvent, ProjectsRequest, ProjectsResponse,
};
use crate::protocol::terminals::{TerminalsRequest, TerminalsResponse};
use crate::protocol::{ErrorCode, Event, OkResponse, ProtocolError, Request, Response};
use crate::search::{self, Searches};
use crate::servers::DevServerState;
use crate::services::{
    AgentRuntime, AuditLog, Browser, DevServers, EventSink, Forge, FsService, GitService, IdeTools,
    KeepAwake, LanguageServers, NetProxy, Opener, ProjectStore, ResourceMonitor, Secrets,
    SessionStore, TerminalService, Unavailable, Worktrees,
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
    git: Arc<dyn GitService>,
    ide_tools: Arc<dyn IdeTools>,
    secrets: Arc<dyn Secrets>,
    audit: Arc<dyn AuditLog>,
    netproxy: Option<Arc<dyn NetProxy>>,
    lsp: Arc<dyn LanguageServers>,
    dev_servers: Arc<dyn DevServers>,
    browser: Arc<dyn Browser>,
    forge: Arc<dyn Forge>,
    resources: Option<Arc<dyn ResourceMonitor>>,
    keep_awake: Arc<dyn KeepAwake>,
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

    pub fn git(mut self, git: Arc<dyn GitService>) -> Self {
        self.git = git;
        self
    }

    pub fn ide_tools(mut self, tools: Arc<dyn IdeTools>) -> Self {
        self.ide_tools = tools;
        self
    }

    /// Where secrets' values are kept: the OS keychain.
    pub fn secrets(mut self, secrets: Arc<dyn Secrets>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Where the audit log is kept; without one nothing is recorded and
    /// `audit.list` answers `unavailable`.
    pub fn audit(mut self, log: Arc<dyn AuditLog>) -> Self {
        self.audit = log;
        self
    }

    /// Proxies sessions' network through per-session netproxies. Without
    /// one, sessions' network is open (`docs/specs/security.md`).
    pub fn netproxy(mut self, netproxy: Arc<dyn NetProxy>) -> Self {
        self.netproxy = Some(netproxy);
        self
    }

    /// Language servers for trusted projects (ADR 0007).
    pub fn lsp(mut self, servers: Arc<dyn LanguageServers>) -> Self {
        self.lsp = servers;
        self
    }

    /// Runs the projects' dev servers in sandbox pods (ADR 0008).
    pub fn dev_servers(mut self, servers: Arc<dyn DevServers>) -> Self {
        self.dev_servers = servers;
        self
    }

    /// A browser per agent session, behind the session's netproxy (ADR 0009).
    pub fn browser(mut self, browser: Arc<dyn Browser>) -> Self {
        self.browser = browser;
        self
    }

    /// Pushes and pull requests through the person's `gh` (ADR 0010).
    pub fn forge(mut self, forge: Arc<dyn Forge>) -> Self {
        self.forge = forge;
        self
    }

    /// Measures agents' memory and CPU and holds queued workers back while
    /// the computer is short of either (`docs/specs/agent-resources.md`).
    pub fn resources(mut self, monitor: Arc<dyn ResourceMonitor>) -> Self {
        self.resources = Some(monitor);
        self
    }

    /// Keeps the computer awake while agents work (`power.*`).
    pub fn keep_awake(mut self, keep_awake: Arc<dyn KeepAwake>) -> Self {
        self.keep_awake = keep_awake;
        self
    }

    pub fn build(self) -> Core {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let kept = self.sessions.clone();
        let core = Core {
            inner: Arc::new(Inner {
                mode: self.mode,
                projects: self.projects,
                fs: self.fs,
                opener: self.opener,
                terminals: self.terminals,
                agents: self.agents,
                dev_servers: DevServerState::new(self.dev_servers, self.netproxy.clone()),
                browser: crate::browser::BrowserState::new(self.browser),
                forge: self.forge,
                resources: crate::resources::Resources::new(self.resources),
                sessions: Arc::new(Sessions::new(
                    self.sessions,
                    self.worktrees.clone(),
                    self.netproxy,
                )),
                worktrees: self.worktrees,
                git: self.git,
                orchestration: Orchestration::new(self.worktrees_dir, kept),
                held: Mutex::new(None),
                open: Mutex::new(BTreeMap::new()),
                searches: Searches::default(),
                secrets: self.secrets,
                changing_secrets: tokio::sync::Mutex::new(()),
                audit: self.audit,
                lsp: self.lsp,
                lsp_servers: crate::lsp::Servers::default(),
                audit_state: crate::audit::State::default(),
                power: crate::power::State::new(self.keep_awake),
            }),
            events,
        };
        core.inner
            .sessions
            .link_ide(self.ide_tools, core.downgrade());
        orchestration::restore(&core);
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
    git: Arc<dyn GitService>,
    orchestration: Orchestration,
    sessions: Arc<Sessions>,
    /// The project held by requests sent through [`Core::handle`] directly.
    held: Mutex<Option<ProjectId>>,
    /// Open projects and how many clients hold each; each is watched.
    open: Mutex<BTreeMap<ProjectId, usize>>,
    searches: Searches,
    secrets: Arc<dyn Secrets>,
    /// Held while secrets or the projects they belong to change, so a
    /// check and the change it guards see the same state.
    changing_secrets: tokio::sync::Mutex<()>,
    audit: Arc<dyn AuditLog>,
    lsp: Arc<dyn LanguageServers>,
    lsp_servers: crate::lsp::Servers,
    audit_state: crate::audit::State,
    dev_servers: DevServerState,
    browser: crate::browser::BrowserState,
    forge: Arc<dyn Forge>,
    resources: crate::resources::Resources,
    power: crate::power::State,
}

/// A [`Core`] that does not keep it alive; sessions hold one so their IDE
/// tool calls can reach the core without a reference cycle.
#[derive(Debug, Clone)]
pub(crate) struct WeakCore {
    inner: std::sync::Weak<Inner>,
    events: broadcast::Sender<Event>,
}

impl WeakCore {
    /// One that never upgrades, for a test host.
    #[cfg(any(test, feature = "fakes"))]
    pub(crate) fn dangling(events: broadcast::Sender<Event>) -> Self {
        Self {
            inner: std::sync::Weak::new(),
            events,
        }
    }

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
            git: Arc::new(Unavailable("git")),
            ide_tools: Arc::new(Unavailable("IDE tools")),
            secrets: Arc::new(Unavailable("keeping secrets")),
            audit: Arc::new(Unavailable("the audit log")),
            netproxy: None,
            lsp: Arc::new(Unavailable("language servers")),
            dev_servers: Arc::new(Unavailable("servers")),
            browser: Arc::new(Unavailable("the agent browser")),
            forge: Arc::new(Unavailable("pull requests")),
            resources: None,
            keep_awake: Arc::new(Unavailable("keeping the computer awake")),
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
            Request::Fs(r) => self.fs(r).await,
            Request::Open(r) => self.open(r).map(|()| Response::Ok(OkResponse::Ok)),
            Request::Terminals(r) => self.terminals(r).await,
            Request::Agents(r) => agents::handle_agents(&self.agents_ctx(), r).await,
            Request::Permissions(PermissionsRequest::Answer {
                id,
                answer,
                always: true,
            }) => crate::policy::answer_always(self, &id, answer)
                .map(|()| Response::Ok(OkResponse::Ok)),
            Request::Permissions(r) => agents::handle_permissions(&self.agents_ctx(), r),
            Request::Review(r) => orchestration::review(self, r).await,
            Request::Orchestration(r) => orchestration::orchestration(self, r).await,
            Request::Search(r) => search::handle(self, r),
            Request::Policy(r) => crate::policy::handle(self, r),
            Request::Secrets(r) => crate::secrets::handle(self, r).await,
            Request::Audit(r) => crate::audit::handle(self, r),
            Request::Netproxy(r) => crate::netproxy::handle(self, r),
            Request::Git(r) => crate::git::handle(self, r).await,
            Request::Lsp(r) => crate::lsp::handle(self, r).await,
            Request::Servers(r) => crate::servers::handle(self, r).await,
            Request::Browser(r) => crate::browser::handle(self, r).await,
            Request::Pr(r) => crate::pr::handle(self, r).await,
            Request::Power(r) => crate::power::handle(self, r),
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

    pub(crate) fn git_service(&self) -> &dyn GitService {
        self.inner.git.as_ref()
    }

    pub(crate) fn fs_service(&self) -> &dyn FsService {
        self.inner.fs.as_ref()
    }

    pub(crate) fn searches(&self) -> &Searches {
        &self.inner.searches
    }

    pub(crate) fn secrets_store(&self) -> Arc<dyn Secrets> {
        self.inner.secrets.clone()
    }

    pub(crate) async fn changing_secrets(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.changing_secrets.lock().await
    }

    pub(crate) fn audit_log(&self) -> &dyn AuditLog {
        self.inner.audit.as_ref()
    }

    pub(crate) fn language_servers(&self) -> &dyn LanguageServers {
        self.inner.lsp.as_ref()
    }

    pub(crate) fn lsp_servers(&self) -> &crate::lsp::Servers {
        &self.inner.lsp_servers
    }

    pub(crate) fn power(&self) -> &crate::power::State {
        &self.inner.power
    }

    pub(crate) fn audit_state(&self) -> &crate::audit::State {
        &self.inner.audit_state
    }

    pub(crate) fn forge(&self) -> &dyn Forge {
        self.inner.forge.as_ref()
    }

    pub(crate) fn dev_servers(&self) -> &DevServerState {
        &self.inner.dev_servers
    }

    pub(crate) fn resources(&self) -> &crate::resources::Resources {
        &self.inner.resources
    }

    pub(crate) fn browser_state(&self) -> &crate::browser::BrowserState {
        &self.inner.browser
    }

    pub(crate) fn project_store(&self) -> &dyn ProjectStore {
        self.inner.projects.as_ref()
    }

    pub(crate) fn agents_ctx(&self) -> agents::Ctx<'_> {
        agents::Ctx {
            sessions: &self.inner.sessions,
            runtime: self.inner.agents.as_ref(),
            projects: self.inner.projects.as_ref(),
            secrets: self.inner.secrets.as_ref(),
            sink: self.sink(),
        }
    }

    async fn terminals(&self, request: TerminalsRequest) -> Result<Response, ProtocolError> {
        let terminals = &self.inner.terminals;
        match request {
            TerminalsRequest::Open { project, size, cwd } => {
                let root = self.root(&project)?;
                let cwd = match cwd {
                    Some(cwd) => self.folder_in(&root, RelPath::parse(&cwd)?).await?,
                    None => root,
                };
                let id = terminals.open(cwd, size, self.sink())?;
                return Ok(Response::Terminals(TerminalsResponse::Opened { id }));
            }
            TerminalsRequest::Input { id, data } => terminals.input(&id, &data)?,
            TerminalsRequest::Resize { id, size } => terminals.resize(&id, size)?,
            TerminalsRequest::Close { id } => terminals.close(&id)?,
            TerminalsRequest::Scrollback { id } => {
                let (data, seq) = terminals.scrollback(&id)?;
                return Ok(Response::Terminals(TerminalsResponse::Scrollback {
                    data,
                    seq,
                }));
            }
        }
        Ok(Response::Ok(OkResponse::Ok))
    }

    /// The absolute path of `path` when it is a folder in the project: its
    /// parent lists it as a directory, so a file or a symlink is refused.
    async fn folder_in(&self, root: &Path, path: RelPath) -> Result<PathBuf, ProtocolError> {
        if path.is_root() {
            return Ok(root.to_path_buf());
        }
        let (parent, name) = path
            .as_str()
            .rsplit_once('/')
            .unwrap_or(("", path.as_str()));
        let listing = self
            .inner
            .fs
            .list_dir(root.to_path_buf(), RelPath::parse(parent)?)
            .await?;
        let is_dir = listing
            .entries
            .iter()
            .any(|e| e.name == name && e.kind == EntryKind::Dir);
        if !is_dir {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("no folder {:?} in the project", path.as_str()),
            ));
        }
        Ok(path.under(root))
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
                let changing = self.changing_secrets().await;
                store.get(&id)?;
                // Its id is never reused, so its secrets could not be
                // listed again: they go first, and a keychain error keeps
                // the project.
                crate::secrets::remove_project(self, &id).await?;
                store.remove(&id)?;
                drop(changing);
                agents::stop_project(&self.agents_ctx(), &id);
                crate::lsp::stop_project(self, &id);
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
                let reviewed = fingerprint.clone().filter(|_| trusted);
                let project = agents::trust(&self.agents_ctx(), &id, trusted, fingerprint).await?;
                if !project.trusted {
                    crate::lsp::stop_project(self, &id);
                }
                // Trusting stores exactly the fingerprint the person reviewed.
                let event = AuditEvent::TrustChanged {
                    trusted: project.trusted,
                    fingerprint: reviewed.filter(|_| project.trusted),
                };
                crate::audit::record(self, &id, None, event);
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

    async fn fs(&self, request: FsRequest) -> Result<Response, ProtocolError> {
        let fs = &self.inner.fs;
        let response = match request {
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
                let saved = path.as_str().to_owned();
                let written = fs.write_file(root, path, text, expected_mtime_ms).await?;
                // The person saved it: never the direct agent's alone.
                self.inner.sessions.direct.person_saved(&project, &saved);
                Ok(FsResponse::Written { mtime_ms: written })
            }
            FsRequest::Create { project, path, dir } => {
                let (root, path) = (self.root(&project)?, editable(&path)?);
                fs.create(root, path.clone(), dir).await?;
                self.person_changed(&project, [&path]);
                return Ok(Response::Ok(OkResponse::Ok));
            }
            FsRequest::Rename { project, from, to } => {
                let root = self.root(&project)?;
                let (from, to) = (editable(&from)?, editable(&to)?);
                not_into_itself(&from, &to)?;
                fs.rename(root, from.clone(), to.clone()).await?;
                self.person_changed(&project, [&from, &to]);
                return Ok(Response::Ok(OkResponse::Ok));
            }
            FsRequest::Copy { project, from, to } => {
                let root = self.root(&project)?;
                let (from, to) = (editable(&from)?, editable(&to)?);
                not_into_itself(&from, &to)?;
                fs.copy(root, from, to.clone()).await?;
                self.person_changed(&project, [&to]);
                return Ok(Response::Ok(OkResponse::Ok));
            }
            FsRequest::Trash { project, paths } => {
                let root = self.root(&project)?;
                let paths = paths
                    .iter()
                    .map(|p| editable(p))
                    .collect::<Result<Vec<_>, _>>()?;
                fs.trash(root, paths.clone()).await?;
                self.person_changed(&project, &paths);
                return Ok(Response::Ok(OkResponse::Ok));
            }
        };
        response.map(Response::Fs)
    }

    /// The person changed these paths: never the direct agent's alone.
    fn person_changed<'a>(
        &self,
        project: &ProjectId,
        paths: impl IntoIterator<Item = &'a RelPath>,
    ) {
        for path in paths {
            self.inner
                .sessions
                .direct
                .person_saved(project, path.as_str());
        }
    }

    fn open(&self, request: OpenRequest) -> Result<(), ProtocolError> {
        match request {
            OpenRequest::External {
                project,
                path,
                line,
            } => {
                let (root, path) = (self.root(&project)?, RelPath::parse(&path)?);
                if path.is_root() {
                    return Err(ProtocolError::new(ErrorCode::BadRequest, "name a file"));
                }
                self.inner.opener.open_in_editor(&root, &path, line)
            }
            OpenRequest::Url { url } => {
                check_web_url(&url)?;
                self.inner.opener.open_url(&url)
            }
            OpenRequest::Reveal { project, path } => {
                let (root, path) = (self.root(&project)?, RelPath::parse(&path)?);
                self.inner.opener.reveal(&root, &path)
            }
        }
    }
}

/// Refuses `to` inside the folder `from`, also with other letter case: on a
/// case-insensitive drive it is the same folder, and elsewhere refusing
/// `src` to `SRC/x` costs little.
fn not_into_itself(from: &RelPath, to: &RelPath) -> Result<(), ProtocolError> {
    let (from, to) = (from.as_str().to_lowercase(), to.as_str().to_lowercase());
    if to.starts_with(&format!("{from}/")) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "cannot put a folder inside itself",
        ));
    }
    Ok(())
}

/// A path the person may create, move, copy or trash: not the project root
/// and nothing in a `.git` folder, which belongs to git.
fn editable(raw: &str) -> Result<RelPath, ProtocolError> {
    let path = RelPath::parse(raw)?;
    if path.is_root() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "name a file or folder",
        ));
    }
    if path
        .as_str()
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            format!("{path} is inside .git"),
        ));
    }
    Ok(path)
}

/// Only web pages may leave the IDE: no `file:`, `javascript:` or custom
/// schemes that could start other programs, and nothing a system handler
/// could read as an option or a second argument.
pub(crate) fn check_web_url(url: &str) -> Result<(), ProtocolError> {
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
        pinned_files: patch.pinned_files.map(all).transpose()?,
        second_group: patch
            .second_group
            .map(|group| {
                Ok::<_, ProtocolError>(EditorGroup {
                    open_files: all(group.open_files)?,
                    active_file: group.active_file.map(norm).transpose()?,
                    pinned_files: all(group.pinned_files)?,
                })
            })
            .transpose()?,
    })
}

#[cfg(test)]
mod tests;
