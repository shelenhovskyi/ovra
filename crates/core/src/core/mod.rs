use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use crate::path::RelPath;
use crate::protocol::fs::{FsRequest, FsResponse};
use crate::protocol::info::{CoreInfo, InfoEvent, InfoRequest, InfoResponse};
use crate::protocol::open::OpenRequest;
use crate::protocol::projects::{
    ProjectId, ProjectStatePatch, ProjectsEvent, ProjectsRequest, ProjectsResponse,
};
use crate::protocol::{ErrorCode, Event, OkResponse, ProtocolError, Request, Response};
use crate::services::{EventSink, FsService, Opener, ProjectStore, Unavailable};

const EVENT_CAPACITY: usize = 1024;

/// Wires services into a [`Core`]. Services left out answer `unavailable`.
#[derive(Debug)]
pub struct CoreBuilder {
    mode: String,
    projects: Arc<dyn ProjectStore>,
    fs: Arc<dyn FsService>,
    opener: Arc<dyn Opener>,
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

    pub fn build(self) -> Core {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Core {
            inner: Arc::new(Inner {
                mode: self.mode,
                projects: self.projects,
                fs: self.fs,
                opener: self.opener,
                active: Mutex::new(None),
            }),
            events,
        }
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
    active: Mutex<Option<ProjectId>>,
}

impl Core {
    /// Starts building a core for a front-end mode (`tauri`, `headless`, ...).
    pub fn builder(mode: impl Into<String>) -> CoreBuilder {
        CoreBuilder {
            mode: mode.into(),
            projects: Arc::new(Unavailable("project store")),
            fs: Arc::new(Unavailable("file access")),
            opener: Arc::new(Unavailable("opening files externally")),
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

    /// Handles one request.
    pub async fn handle(&self, request: Request) -> Result<Response, ProtocolError> {
        match request {
            Request::Info(InfoRequest::Get) => Ok(Response::Info(InfoResponse::Info(CoreInfo {
                name: "ovra".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                mode: self.inner.mode.clone(),
            }))),
            Request::Projects(r) => self.projects(r),
            Request::Fs(r) => self.fs(r).await.map(Response::Fs),
            Request::Open(r) => self.open(r).map(|()| Response::Ok(OkResponse::Ok)),
        }
    }

    fn active(&self) -> std::sync::MutexGuard<'_, Option<ProjectId>> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn projects(&self, request: ProjectsRequest) -> Result<Response, ProtocolError> {
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
                let mut active = self.active();
                if active.as_ref() == Some(&id) {
                    self.inner.fs.unwatch(&id);
                    *active = None;
                }
                drop(active);
                self.emit_projects_changed();
                Ok(ProjectsResponse::List(store.list()?))
            }
            ProjectsRequest::Open { id } => {
                let project = store.touch(&id)?;
                let mut active = self.active();
                if active.as_ref() != Some(&id) {
                    if let Some(previous) = active.take() {
                        self.inner.fs.unwatch(&previous);
                    }
                    self.inner
                        .fs
                        .watch(id.clone(), PathBuf::from(&project.root), self.sink())?;
                    *active = Some(id);
                }
                drop(active);
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
        let active = self.active().clone();
        self.emit(ProjectsEvent::Changed { active });
    }

    fn root(&self, project: &ProjectId) -> Result<PathBuf, ProtocolError> {
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
