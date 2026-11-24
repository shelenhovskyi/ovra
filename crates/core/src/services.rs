//! Seams between the core and the outside world. Each trait has a real
//! implementation in its own crate and a fake in [`crate::fakes`].

use std::fmt::Debug;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::sync::broadcast;

use crate::path::RelPath;
use crate::protocol::fs::{DirListing, FileContent};
use crate::protocol::projects::{Project, ProjectId, ProjectState};
use crate::protocol::{ErrorCode, Event, ProtocolError};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Lets services push events to every connected front end.
#[derive(Debug, Clone)]
pub struct EventSink(pub(crate) broadcast::Sender<Event>);

impl EventSink {
    /// Pushes an event. Having no subscribers is fine.
    pub fn emit(&self, event: impl Into<Event>) {
        let _ = self.0.send(event.into());
    }
}

/// The list of projects, persisted across restarts (implemented by `ovra-store`).
pub trait ProjectStore: Send + Sync + Debug {
    /// All projects, most recently opened first.
    fn list(&self) -> Result<Vec<Project>>;
    /// Adds a project for an absolute folder path, or returns the existing one.
    fn add(&self, root: &Path) -> Result<Project>;
    /// Forgets a project. `not_found` if it does not exist.
    fn remove(&self, id: &ProjectId) -> Result<()>;
    /// `not_found` if it does not exist.
    fn get(&self, id: &ProjectId) -> Result<Project>;
    /// Records that the project was opened now and returns it.
    fn touch(&self, id: &ProjectId) -> Result<Project>;
    /// The saved state, or the default if none was saved. `not_found` if the
    /// project does not exist. Removing a project removes its state.
    fn state(&self, id: &ProjectId) -> Result<ProjectState>;
    /// Replaces the saved state. Paths are already validated.
    fn set_state(&self, id: &ProjectId, state: &ProjectState) -> Result<()>;
}

/// File access, `.gitignore` handling, git status and watching
/// (implemented by `ovra-fs-index`). Paths are already validated.
pub trait FsService: Send + Sync + Debug {
    fn list_dir(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<DirListing>>;
    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>>;
    /// Returns the new modification time in Unix milliseconds.
    fn write_file(
        &self,
        root: PathBuf,
        path: RelPath,
        text: String,
        expected_mtime_ms: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>>;
    /// Starts emitting `fs.changed` / `fs.git_changed` for a project.
    fn watch(&self, project: ProjectId, root: PathBuf, sink: EventSink) -> Result<()>;
    fn unwatch(&self, project: &ProjectId);
}

/// Hands files to other applications (implemented by `ovra-opener`).
pub trait Opener: Send + Sync + Debug {
    /// Opens `file` in the user's external editor, at `line` when supported.
    fn open_in_editor(&self, file: &Path, line: Option<u32>) -> Result<()>;
    /// Opens a web page in the system browser. The core passes only
    /// `http://` and `https://` URLs.
    fn open_url(&self, url: &str) -> Result<()>;
}

/// Placeholder for services a front end has not wired in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Unavailable(pub(crate) &'static str);

impl Unavailable {
    fn err<T>(&self) -> Result<T> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!("{} is not available in this build", self.0),
        ))
    }
}

impl ProjectStore for Unavailable {
    fn list(&self) -> Result<Vec<Project>> {
        self.err()
    }
    fn add(&self, _: &Path) -> Result<Project> {
        self.err()
    }
    fn remove(&self, _: &ProjectId) -> Result<()> {
        self.err()
    }
    fn get(&self, _: &ProjectId) -> Result<Project> {
        self.err()
    }
    fn touch(&self, _: &ProjectId) -> Result<Project> {
        self.err()
    }
    fn state(&self, _: &ProjectId) -> Result<ProjectState> {
        self.err()
    }
    fn set_state(&self, _: &ProjectId, _: &ProjectState) -> Result<()> {
        self.err()
    }
}

impl FsService for Unavailable {
    fn list_dir(&self, _: PathBuf, _: RelPath) -> BoxFuture<'_, Result<DirListing>> {
        Box::pin(async move { self.err() })
    }
    fn read_file(&self, _: PathBuf, _: RelPath) -> BoxFuture<'_, Result<FileContent>> {
        Box::pin(async move { self.err() })
    }
    fn write_file(
        &self,
        _: PathBuf,
        _: RelPath,
        _: String,
        _: Option<u64>,
    ) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async move { self.err() })
    }
    fn watch(&self, _: ProjectId, _: PathBuf, _: EventSink) -> Result<()> {
        self.err()
    }
    fn unwatch(&self, _: &ProjectId) {}
}

impl Opener for Unavailable {
    fn open_in_editor(&self, _: &Path, _: Option<u32>) -> Result<()> {
        self.err()
    }
    fn open_url(&self, _: &str) -> Result<()> {
        self.err()
    }
}
