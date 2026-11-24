//! Seams between the core and the outside world. Each trait has a real
//! implementation in its own crate and a fake in [`crate::fakes`].

use std::fmt::Debug;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::sync::broadcast;

pub use crate::agents::AgentHost;
use crate::path::RelPath;
use crate::protocol::agents::{
    AgentKind, AgentSettings, AgentSetup, ProjectAgentConfig, SessionId, SessionMode, SetupState,
};
use crate::protocol::fs::{DirListing, FileContent, FileList};
use crate::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use crate::protocol::terminals::{TermSize, TerminalId};
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
    /// Writes the fields present in `patch` atomically, keeping the others.
    /// Paths are already validated.
    fn update_state(&self, id: &ProjectId, patch: ProjectStatePatch) -> Result<()>;
    /// Records what the user trusted (`None`: stops trusting) and returns
    /// the project.
    fn set_trusted(&self, id: &ProjectId, trusted: Option<ProjectAgentConfig>) -> Result<Project>;
    /// What the user trusted, if they trust the project.
    fn trusted_config(&self, id: &ProjectId) -> Result<Option<ProjectAgentConfig>>;
    /// The agent settings, or the defaults if none were saved.
    fn agent_settings(&self) -> Result<AgentSettings>;
    fn set_agent_settings(&self, settings: AgentSettings) -> Result<()>;
}

/// File access, `.gitignore` handling, git status and watching
/// (implemented by `ovra-fs-index`). Paths are already validated.
pub trait FsService: Send + Sync + Debug {
    fn list_dir(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<DirListing>>;
    fn read_file(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<FileContent>>;
    /// A page of the project's non-ignored files; `limit` is already capped.
    fn list_files(&self, root: PathBuf, offset: u32, limit: u32)
    -> BoxFuture<'_, Result<FileList>>;
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

/// Shells in pseudo-terminals (implemented by `ovra-pty`).
pub trait TerminalService: Send + Sync + Debug {
    /// Starts the user's shell in `cwd` and emits `terminals.output` and
    /// `terminals.exited` for it through `sink`.
    fn open(&self, cwd: PathBuf, size: TermSize, sink: EventSink) -> Result<TerminalId>;
    fn input(&self, id: &TerminalId, data: &str) -> Result<()>;
    fn resize(&self, id: &TerminalId, size: TermSize) -> Result<()>;
    /// Ends the shell; `terminals.exited` follows.
    fn close(&self, id: &TerminalId) -> Result<()>;
}

/// What a runtime needs to start one session.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub session: SessionId,
    pub agent: AgentKind,
    pub mode: SessionMode,
    /// Absolute path the agent works in: the project root in direct mode.
    pub cwd: PathBuf,
    pub prompt: String,
    /// The executable the user chose for this agent, if any.
    pub executable: Option<PathBuf>,
    /// Whether the project's own agent configuration may load: the user
    /// trusts the project and it has not changed since. Otherwise only the
    /// user's own settings load.
    pub trusted: bool,
}

/// Starts agent sessions (implemented by `ovra-agent-runtime`). One runtime
/// serves every session of the agents it supports.
pub trait AgentRuntime: Send + Sync + Debug {
    /// Starts a session and returns at once; the agent then works in the
    /// background and reports through `host` until it ends.
    fn start(&self, spec: AgentSpec, host: AgentHost) -> Result<Box<dyn RunningAgent>>;
    /// Checks that the agent can run here; `path` is the executable the user
    /// chose. Runtimes that need nothing installed are always ready.
    fn setup(&self, path: Option<PathBuf>) -> BoxFuture<'_, AgentSetup> {
        Box::pin(async move {
            AgentSetup {
                path: path.map(|p| p.to_string_lossy().into_owned()),
                executable: None,
                version: None,
                state: SetupState::Ready,
            }
        })
    }
    /// What the project at `root` would load once trusted. Runtimes without
    /// per-project configuration find nothing.
    fn project_config(&self, _root: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async { Ok(ProjectAgentConfig::default()) })
    }
}

/// A started session, owned by the core.
pub trait RunningAgent: Send + Sync + Debug {
    /// Queues the next user message.
    fn send(&self, text: String) -> Result<()>;
    /// Stops the current turn; the session then waits for the next message.
    fn interrupt(&self) -> Result<()> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "this agent cannot be interrupted",
        ))
    }
    /// Ends the session. The runtime reports `stopped` through the host.
    fn stop(&self);
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
    fn update_state(&self, _: &ProjectId, _: ProjectStatePatch) -> Result<()> {
        self.err()
    }
    fn set_trusted(&self, _: &ProjectId, _: Option<ProjectAgentConfig>) -> Result<Project> {
        self.err()
    }
    fn trusted_config(&self, _: &ProjectId) -> Result<Option<ProjectAgentConfig>> {
        self.err()
    }
    fn agent_settings(&self) -> Result<AgentSettings> {
        self.err()
    }
    fn set_agent_settings(&self, _: AgentSettings) -> Result<()> {
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
    fn list_files(&self, _: PathBuf, _: u32, _: u32) -> BoxFuture<'_, Result<FileList>> {
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

impl TerminalService for Unavailable {
    fn open(&self, _: PathBuf, _: TermSize, _: EventSink) -> Result<TerminalId> {
        self.err()
    }
    fn input(&self, _: &TerminalId, _: &str) -> Result<()> {
        self.err()
    }
    fn resize(&self, _: &TerminalId, _: TermSize) -> Result<()> {
        self.err()
    }
    fn close(&self, _: &TerminalId) -> Result<()> {
        self.err()
    }
}

impl AgentRuntime for Unavailable {
    fn start(&self, _: AgentSpec, _: AgentHost) -> Result<Box<dyn RunningAgent>> {
        self.err()
    }
    fn setup(&self, path: Option<PathBuf>) -> BoxFuture<'_, AgentSetup> {
        Box::pin(async move {
            AgentSetup {
                path: path.map(|p| p.to_string_lossy().into_owned()),
                executable: None,
                version: None,
                state: SetupState::Broken {
                    message: format!("{} is not available in this build", self.0),
                },
            }
        })
    }
    fn project_config(&self, _: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async move { self.err() })
    }
}
