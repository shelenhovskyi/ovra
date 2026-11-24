//! Seams between the core and the outside world. Each trait has a real
//! implementation in its own crate and a fake in [`crate::fakes`].

use std::fmt::Debug;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::broadcast;

pub use crate::agents::AgentHost;
use crate::path::RelPath;
use crate::protocol::agents::{
    AgentKind, AgentMessage, AgentSettings, AgentSetup, ProjectAgentConfig, SessionId, SessionInfo,
    SessionMode, SetupState,
};
use crate::protocol::fs::{DirListing, FileContent, FileList};
use crate::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use crate::protocol::review::ChangeKind;
use crate::protocol::search::{SearchId, SearchQuery};
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

    /// Events not yet received by every subscriber. A service that can wait
    /// (terminal output) holds back while this is high, so that subscribers
    /// do not fall behind and miss events.
    pub fn backlog(&self) -> usize {
        self.0.len()
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
    /// Starts searching the contents of the files `list_files` lists and
    /// returns at once. Results go to `sink` as `search.results` events,
    /// then one `search.done`. Fails with `bad_request`, emitting nothing,
    /// for a query it cannot run (an invalid regex or glob). The pattern is
    /// already checked to be non-empty.
    fn search(
        &self,
        id: SearchId,
        root: PathBuf,
        query: SearchQuery,
        sink: EventSink,
        cancel: SearchCancel,
    ) -> Result<()>;
    /// Starts emitting `fs.changed` / `fs.git_changed` for a project.
    fn watch(&self, project: ProjectId, root: PathBuf, sink: EventSink) -> Result<()>;
    fn unwatch(&self, project: &ProjectId);
}

/// Set by the core when a client cancels a search; the search stops soon
/// after. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct SearchCancel(Arc<AtomicBool>);

impl SearchCancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Whether no clone is left but this one: the search ended.
    pub(crate) fn is_alone(&self) -> bool {
        Arc::strong_count(&self.0) == 1
    }
}

/// Hands files to other applications (implemented by `ovra-opener`).
pub trait Opener: Send + Sync + Debug {
    /// Opens the file `path` under `root` in the user's external editor, at
    /// `line` when supported. Symlinks are resolved first, and a file that
    /// resolves outside `root` is refused (`Forbidden`), so a symlink in a
    /// project cannot hand an outside file to the editor. This does not
    /// stop the system's default handler from running a program that is
    /// inside the project.
    fn open_in_editor(&self, root: &Path, path: &RelPath, line: Option<u32>) -> Result<()>;
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
    /// Ends the shell if it still runs, and forgets the terminal;
    /// `terminals.exited` follows for a running shell.
    fn close(&self, id: &TerminalId) -> Result<()>;
    /// The kept tail of the output and the `seq` of the last output event
    /// it contains. Answers for an ended terminal until it is closed.
    fn scrollback(&self, id: &TerminalId) -> Result<(String, u64)>;
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
    /// Ovra's IDE tools for the session, offered as the in-process MCP
    /// server `ovra` (`docs/specs/orchestration.md`); `None` for none.
    pub tools: Option<ToolRole>,
    /// Whether the project's own agent configuration may load: the user
    /// trusts the project and it has not changed since. Otherwise only the
    /// user's own settings load.
    pub trusted: bool,
    /// The agent's own id of a conversation to continue, as the runtime
    /// reported it through [`AgentHost::agent_session`]; `None` starts anew.
    pub resume: Option<String>,
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
                billing: None,
                api_key_in_env: false,
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

/// A session as kept between core runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub info: SessionInfo,
    /// The agent's own id for the conversation (Claude Code's session id).
    pub agent_session: Option<String>,
}

/// Keeps agent sessions and their transcripts across restarts (implemented
/// by `ovra-store`). The core keeps working when a call fails.
pub trait SessionStore: Send + Sync + Debug {
    /// Every kept session, oldest first, with at most the last
    /// `max_messages` messages of its transcript.
    fn load(&self, max_messages: usize) -> Result<Vec<(StoredSession, Vec<AgentMessage>)>>;
    /// Adds a session or replaces its metadata.
    fn save(&self, session: &StoredSession) -> Result<()>;
    /// Adds a message to the end of a saved session's transcript.
    fn append(&self, session: &SessionId, message: &AgentMessage) -> Result<()>;
    /// Forgets a session and its transcript.
    fn remove(&self, session: &SessionId) -> Result<()>;
    /// The orchestration state (tasks and review items) as the core last
    /// saved it, a JSON document the store keeps as is; `None` if none.
    fn load_orchestration(&self) -> Result<Option<String>>;
    /// Replaces the saved orchestration state.
    fn save_orchestration(&self, json: &str) -> Result<()>;
}

/// Which IDE tools a session gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRole {
    Coordinator,
    Worker,
}

/// Answers the MCP messages an agent sends to Ovra's IDE tools (implemented
/// by `ovra-mcp-server`). Runtimes pass them through [`AgentHost::call_ide`].
pub trait IdeTools: Send + Sync + Debug {
    /// Answers one JSON-RPC message from `caller`; `None` for a notification.
    /// `core` is the core the session runs in, for the requests a tool makes.
    fn handle(
        &self,
        core: crate::Core,
        caller: ToolCaller,
        message: serde_json::Value,
    ) -> BoxFuture<'_, Option<serde_json::Value>>;
}

/// The session behind an IDE tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCaller {
    pub session: SessionId,
    pub project: ProjectId,
    pub role: ToolRole,
}

/// A worktree made for an isolated session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    /// The commit it started from.
    pub base: String,
}

/// One changed file between a base commit and a working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    pub path: String,
    pub kind: ChangeKind,
    /// The old path of a rename.
    pub from: Option<String>,
    pub additions: u32,
    pub deletions: u32,
}

/// Git worktrees, snapshots and diffs for review (implemented by
/// `ovra-worktree`; ADR 0005). Directories are absolute; paths are relative
/// to them and already validated.
pub trait Worktrees: Send + Sync + Debug {
    /// Records the working tree of `dir`, uncommitted and untracked files
    /// included (ignored files excluded), as a commit, without touching HEAD,
    /// the index or the stash. Returns the commit id.
    fn snapshot(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>>;
    /// Adds a worktree of the repository at `root` at `dir`, on a new
    /// `branch` that starts at `base`.
    fn create(
        &self,
        root: PathBuf,
        dir: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>>;
    /// Files that differ between `base` and the working tree of `dir`.
    fn changes(&self, dir: PathBuf, base: String) -> BoxFuture<'_, Result<Vec<Delta>>>;
    /// The unified diff of `paths`, `base` to the working tree, in one
    /// piece (one call however many files: reviews can be large).
    fn patch(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<String>>;
    /// Puts `paths` back as they are in `base` (deleting files `base` lacks).
    fn restore(&self, dir: PathBuf, base: String, paths: Vec<RelPath>)
    -> BoxFuture<'_, Result<()>>;
    /// Copies `paths` from the working tree of `from` into `into`. A file in
    /// `into` that no longer matches `base` is skipped and returned as a
    /// conflict.
    fn apply(
        &self,
        from: PathBuf,
        base: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>>;
    /// Commits exactly `paths` in `dir` with `message`; returns the commit id.
    fn commit(
        &self,
        dir: PathBuf,
        paths: Vec<RelPath>,
        message: String,
    ) -> BoxFuture<'_, Result<String>>;
    /// Removes the worktree at `dir` from the repository at `root`. Its
    /// branch is kept.
    fn remove(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>>;
    /// Lets go of every snapshot of the repository at `root` (its main
    /// checkout and its worktrees) except those in `keep`, so that git can
    /// collect them. The core calls it when reviews no longer need them.
    fn prune_snapshots(&self, root: PathBuf, keep: Vec<String>) -> BoxFuture<'_, Result<()>>;
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
    fn search(
        &self,
        _: SearchId,
        _: PathBuf,
        _: SearchQuery,
        _: EventSink,
        _: SearchCancel,
    ) -> Result<()> {
        self.err()
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
    fn open_in_editor(&self, _: &Path, _: &RelPath, _: Option<u32>) -> Result<()> {
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
    fn scrollback(&self, _: &TerminalId) -> Result<(String, u64)> {
        self.err()
    }
}

impl SessionStore for Unavailable {
    fn load(&self, _: usize) -> Result<Vec<(StoredSession, Vec<AgentMessage>)>> {
        Ok(Vec::new())
    }
    fn save(&self, _: &StoredSession) -> Result<()> {
        Ok(())
    }
    fn append(&self, _: &SessionId, _: &AgentMessage) -> Result<()> {
        Ok(())
    }
    fn remove(&self, _: &SessionId) -> Result<()> {
        Ok(())
    }
    fn load_orchestration(&self) -> Result<Option<String>> {
        Ok(None)
    }
    fn save_orchestration(&self, _: &str) -> Result<()> {
        Ok(())
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
                billing: None,
                api_key_in_env: false,
            }
        })
    }
    fn project_config(&self, _: PathBuf) -> BoxFuture<'_, Result<ProjectAgentConfig>> {
        Box::pin(async move { self.err() })
    }
}

impl Worktrees for Unavailable {
    fn snapshot(&self, _: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.err() })
    }
    fn create(
        &self,
        _: PathBuf,
        _: PathBuf,
        _: String,
        _: String,
    ) -> BoxFuture<'_, Result<WorktreeInfo>> {
        Box::pin(async move { self.err() })
    }
    fn changes(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move { self.err() })
    }
    fn patch(&self, _: PathBuf, _: String, _: Vec<RelPath>) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.err() })
    }
    fn restore(&self, _: PathBuf, _: String, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.err() })
    }
    fn apply(
        &self,
        _: PathBuf,
        _: String,
        _: PathBuf,
        _: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async move { self.err() })
    }
    fn commit(&self, _: PathBuf, _: Vec<RelPath>, _: String) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.err() })
    }
    fn remove(&self, _: PathBuf, _: PathBuf) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.err() })
    }
    fn prune_snapshots(&self, _: PathBuf, _: Vec<String>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.err() })
    }
}

impl IdeTools for Unavailable {
    fn handle(
        &self,
        _: crate::Core,
        _: ToolCaller,
        message: serde_json::Value,
    ) -> BoxFuture<'_, Option<serde_json::Value>> {
        Box::pin(async move {
            let id = message.get("id")?.clone();
            Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("{} is not available in this build", self.0)},
            }))
        })
    }
}
