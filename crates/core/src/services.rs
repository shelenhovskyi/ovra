//! Seams between the core and the outside world. Each trait has a real
//! implementation in its own crate and a fake in [`crate::fakes`].

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Debug;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::broadcast;

pub use crate::agents::AgentHost;
pub use crate::lsp::ServerHost;
use crate::path::RelPath;
use crate::protocol::agents::{
    AgentKind, AgentMessage, AgentSettings, AgentSetup, ProjectAgentConfig, SessionId, SessionInfo,
    SessionMode, SetupState,
};
use crate::protocol::audit::{AuditEntry, AuditEvent};
use crate::protocol::fs::{DirListing, FileContent, FileList};
use crate::protocol::git::{
    Blame, BranchInfo, CommitInfo, Committed, FileDiff, GitFileText, GitRev, Hunk, RepoStatus,
};
use crate::protocol::lsp::KnownServer;
use crate::protocol::netproxy::{Allowlist, NetproxyEvent};
use crate::protocol::policy::Policy;
use crate::protocol::projects::{Project, ProjectId, ProjectState, ProjectStatePatch};
use crate::protocol::review::ChangeKind;
use crate::protocol::search::{SearchId, SearchQuery};
use crate::protocol::secrets::{SecretInfo, SecretStoreStatus, SecretValue};
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
    /// The project's policy (`docs/specs/security.md`); empty if none was set.
    fn policy(&self, id: &ProjectId) -> Result<Policy>;
    fn set_policy(&self, id: &ProjectId, policy: &Policy) -> Result<()>;
    /// The hosts the project's sessions may reach besides the netproxy's
    /// base list; empty if none were set. Hosts are already normalized.
    fn allowlist(&self, id: &ProjectId) -> Result<Allowlist>;
    fn set_allowlist(&self, id: &ProjectId, allowlist: &Allowlist) -> Result<()>;
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
    /// What the session may touch if the runtime sandboxes it.
    pub sandbox: SandboxProfile,
}

/// A program to start: plain data, so the core can pass it around without
/// starting anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Variables to set (`Some`) or remove (`None`); the rest is inherited.
    pub env: Vec<(OsString, Option<OsString>)>,
    pub cwd: PathBuf,
}

/// What one sandboxed session may touch (`docs/specs/security.md`). The
/// platform adds what only it knows: the agent's own state, the system,
/// temporary files, the protected paths of `ovra-sandbox`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProfile {
    /// The session's folder, readable and writable.
    pub work_dir: PathBuf,
    /// More folders it may read but not write.
    pub read: Vec<PathBuf>,
    /// More folders it may write.
    pub write: Vec<PathBuf>,
    pub network: ProfileNetwork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileNetwork {
    Blocked,
    /// Only through the session's netproxy at `address`.
    Proxy {
        address: String,
        /// The CA bundle the sandbox trusts, if the proxy adds secrets:
        /// the system's and the person's roots plus the session's own CA
        /// (`docs/specs/secrets.md`). The sandbox makes it readable and
        /// points `SSL_CERT_FILE` and the like at it.
        ca_bundle: Option<PathBuf>,
    },
    /// Not limited: no netproxy is wired in.
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    Available { name: String },
    Unavailable { reason: String },
}

/// Runs agent processes inside a sandbox (implemented by the platform
/// crates through `ovra-sandbox`). Wired into the agent runtime, which
/// reports what it did through [`AgentHost::sandboxed`].
pub trait Sandbox: Send + Sync + Debug {
    fn status(&self) -> SandboxStatus;
    /// The command line that runs `line` inside `profile`. Starts nothing.
    fn wrap(&self, profile: &SandboxProfile, line: CommandLine) -> Result<CommandLine>;
    /// The command line that runs a language server's `line` inside
    /// `profile` (ADR 0007). Starts nothing. A sandbox that cannot do this
    /// yet answers `unavailable`, and the server does not start.
    fn wrap_server(&self, profile: &ServerProfile, line: CommandLine) -> Result<CommandLine> {
        let _ = (profile, line);
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "this sandbox cannot run language servers yet",
        ))
    }
}

/// What one language server may touch (ADR 0007, `docs/specs/lsp.md`).
/// Unlike an agent, a server never writes the project. The platform adds
/// what it adds for agents except the agent's own state: the system,
/// toolchains, the user's package caches (read-only), temporary files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerProfile {
    /// The project root: readable, never writable.
    pub project: PathBuf,
    /// The server's own folder for caches and build output, writable.
    pub cache_dir: PathBuf,
    /// More folders it may read.
    pub read: Vec<PathBuf>,
    pub network: ProfileNetwork,
}

/// What the core asks a [`LanguageServers`] service to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    /// The project, so the service keeps caches per project.
    pub project: ProjectId,
    /// The project root, absolute: the server's working folder.
    pub root: PathBuf,
    /// The editor's language id (`rust`, `typescript`, ...).
    pub language: String,
}

/// A server that started.
#[derive(Debug)]
pub struct StartedServer {
    pub process: Box<dyn RunningServer>,
    /// The server's name, such as `rust-analyzer`.
    pub name: String,
    /// The sandbox it runs in; `None` when it runs unsandboxed.
    pub sandbox: Option<String>,
}

/// Language servers found on the machine and run through the sandbox
/// (ADR 0007; implemented by `ovra-lsp`). Trust, URIs and the requests a
/// server makes are the core's job; this is the process.
pub trait LanguageServers: Send + Sync + Debug {
    /// The servers Ovra knows, one per language, and where each was found.
    fn known(&self) -> BoxFuture<'_, Vec<KnownServer>>;
    /// The sandbox servers run in; `None` when they run unsandboxed.
    fn sandbox(&self) -> Option<String>;
    /// Starts the server for `spec.language` in the project root, through
    /// the sandbox. `not_found` when no server is known or installed for
    /// the language; an error when the sandbox is available but cannot
    /// wrap the server. Each message the server writes goes to
    /// [`ServerHost::message`] in order, held back while
    /// [`ServerHost::busy`]; [`ServerHost::exited`] comes once, last.
    fn start(&self, spec: ServerSpec, host: ServerHost) -> BoxFuture<'_, Result<StartedServer>>;
}

/// One running language server.
pub trait RunningServer: Send + Sync + Debug {
    /// Writes one message to the server.
    fn send(&self, message: serde_json::Value) -> Result<()>;
    /// Kills the server if it still runs; [`ServerHost::exited`] follows.
    /// The core asks it to shut down politely first.
    fn stop(&self);
}

/// Per-session HTTP proxies that are a sandbox's only way out
/// (`docs/specs/security.md`; implemented by `ovra-netproxy`). Each allows
/// [`NetProxy::base`] plus the project's allowlist and refuses everything
/// else, loopback and private addresses whatever the name.
pub trait NetProxy: Send + Sync + Debug {
    /// What every session may reach: the hosts `claude` itself needs.
    fn base(&self) -> Allowlist;
    /// Starts the proxy of `session`, which allows `base()` and `allow`,
    /// until [`NetProxy::stop`]. It adds each of `secrets` (the project's,
    /// fixed for this run) to requests for its host
    /// (`docs/specs/secrets.md`). Returns what [`ProfileNetwork::Proxy`]
    /// needs.
    fn start(
        &self,
        session: SessionId,
        allow: Allowlist,
        refusals: NetReporter,
        secrets: Vec<SecretInfo>,
    ) -> BoxFuture<'_, Result<NetStarted>>;
    /// Replaces a running proxy's project allowlist; new connections follow
    /// it. Does nothing for a session without a proxy.
    fn update(&self, session: &SessionId, allow: Allowlist);
    /// Stops the proxy and its connections. Does nothing without one.
    fn stop(&self, session: &SessionId);
}

/// A started netproxy: its address and, if it adds secrets, the CA
/// bundle for the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetStarted {
    pub address: String,
    pub ca_bundle: Option<PathBuf>,
}

/// How a netproxy reports one session's refusals to the core. Cheap to clone.
#[derive(Debug, Clone)]
pub struct NetReporter {
    project: ProjectId,
    session: SessionId,
    sink: EventSink,
    /// Where refusals are written to the audit log; `None` in tests of
    /// other crates.
    core: Option<crate::core::WeakCore>,
    /// Shared by the clones of one session's reporter.
    limit: Arc<std::sync::Mutex<RefusalWindow>>,
}

/// Refusals recorded in the current minute, and those left out.
#[derive(Debug)]
struct RefusalWindow {
    started: std::time::Instant,
    recorded: u32,
    left_out: u32,
}

/// Refusals one session records per minute; an agent in a loop must not
/// fill the audit log or flood the UI.
const MAX_REFUSALS_PER_MINUTE: u32 = 30;
/// Hosts and reasons are cut to this many characters.
const MAX_REFUSAL_TEXT: usize = 255;

impl NetReporter {
    pub fn new(project: ProjectId, session: SessionId, sink: EventSink) -> Self {
        Self {
            project,
            session,
            sink,
            core: None,
            limit: Arc::new(std::sync::Mutex::new(RefusalWindow {
                started: std::time::Instant::now(),
                recorded: 0,
                left_out: 0,
            })),
        }
    }

    /// The session's project, whose secrets the proxy adds.
    pub fn project(&self) -> &ProjectId {
        &self.project
    }

    /// The proxy added secret `name` to a request for `host`: written to
    /// the audit log as `SecretUsed`, never with the value.
    pub fn secret_used(&self, name: &str, host: &str) {
        if let Some(core) = self.core.as_ref().and_then(|c| c.upgrade()) {
            crate::audit::record(
                &core,
                &self.project,
                Some(&self.session),
                AuditEvent::SecretUsed {
                    name: name.to_owned(),
                    host: host.to_owned(),
                },
            );
        }
    }

    pub(crate) fn with_core(mut self, core: Option<crate::core::WeakCore>) -> Self {
        self.core = core;
        self
    }

    /// The proxy refused a connection to `host` (`host:port`): written to
    /// the audit log as `NetworkRefused` and shown to every front end, at
    /// most 30 a minute for one session. The first refusal after a full
    /// minute says how many were left out.
    pub fn refused(&self, host: &str, reason: &str) {
        let left_out = {
            let mut window = self
                .limit
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut left_out = None;
            if window.started.elapsed() >= std::time::Duration::from_secs(60) {
                left_out = Some(window.left_out).filter(|n| *n > 0);
                *window = RefusalWindow {
                    started: std::time::Instant::now(),
                    recorded: 0,
                    left_out: 0,
                };
            }
            if window.recorded >= MAX_REFUSALS_PER_MINUTE {
                window.left_out += 1;
                return;
            }
            window.recorded += 1;
            left_out
        };
        if let Some(n) = left_out {
            self.record(
                "(more)",
                &format!("{n} more refusals in the last minute were left out"),
            );
        }
        self.record(host, reason);
    }

    fn record(&self, host: &str, reason: &str) {
        let cut = |text: &str| text.chars().take(MAX_REFUSAL_TEXT).collect::<String>();
        let (host, reason) = (cut(host), cut(reason));
        let (host, reason) = (host.as_str(), reason.as_str());
        if let Some(core) = self.core.as_ref().and_then(|c| c.upgrade()) {
            crate::audit::record(
                &core,
                &self.project,
                Some(&self.session),
                AuditEvent::NetworkRefused {
                    host: host.to_owned(),
                    reason: reason.to_owned(),
                },
            );
        }
        self.sink.emit(NetproxyEvent::Refused {
            project: self.project.clone(),
            session: self.session.clone(),
            host: host.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

/// The append-only audit log (`docs/specs/security.md`; stored by
/// `ovra-store` from task 4.E).
pub trait AuditLog: Send + Sync + Debug {
    fn append(
        &self,
        project: &ProjectId,
        session: Option<&SessionId>,
        event: AuditEvent,
    ) -> Result<AuditEntry>;
    /// Up to `limit` entries of `project` after `after` (a `seq`), oldest first.
    fn list(
        &self,
        project: &ProjectId,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AuditEntry>>;
}

/// Tokens in the OS keychain, each bound to one project, host and header
/// (`docs/specs/secrets.md`; implemented by `ovra-secrets`). Calls may
/// block on the keychain.
pub trait Secrets: Send + Sync + Debug {
    /// Whether values can be stored, and where.
    fn status(&self) -> SecretStoreStatus;
    /// The project's secrets without values, by name.
    fn list(&self, project: &ProjectId) -> Result<Vec<SecretInfo>>;
    /// Stores or replaces a secret. The core has checked every field.
    /// `unavailable` without a keychain; never falls back to a file.
    fn set(&self, project: &ProjectId, secret: SecretInfo, value: SecretValue) -> Result<()>;
    /// `not_found` if the project has no such secret.
    fn remove(&self, project: &ProjectId, name: &str) -> Result<()>;
    /// Removes every secret of a project the person removes.
    fn remove_project(&self, project: &ProjectId) -> Result<()>;
    /// The value, for the netproxy and the audit log's redaction: no
    /// protocol method reaches it.
    fn value(&self, project: &ProjectId, name: &str) -> Result<SecretValue>;
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
    /// Names the content of the new side (empty for a deletion): equal
    /// versions mean equal content and mode. Review fingerprints use it.
    pub version: String,
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
    /// The working tree of `dir` as it is now, for `diff` and `patches`
    /// right away. Unlike a snapshot it is not kept: `git gc` may drop it.
    fn look(&self, dir: PathBuf) -> BoxFuture<'_, Result<String>>;
    /// The files of `dir`'s repository that run code or change what git
    /// does and that no review diff shows: its config, `info/attributes`
    /// and `info/exclude`, and the hooks folder (`core.hooksPath`, or
    /// `.git/hooks`). Compared at review time with what they were when the
    /// session started (SEC-5).
    fn git_files(&self, dir: PathBuf) -> BoxFuture<'_, Result<GitFiles>>;
    /// Files that differ between two commits (snapshots) or looks of the
    /// repository at `dir`; the working tree is not read.
    fn diff(&self, dir: PathBuf, from: String, to: String) -> BoxFuture<'_, Result<Vec<Delta>>>;
    /// The unified diff of `paths`, `base` to the working tree, in one
    /// piece (one call however many files: reviews can be large).
    fn patch(
        &self,
        dir: PathBuf,
        base: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<String>>;
    /// The unified diff of each of `paths` between two commits or looks,
    /// by path (empty without changes), from one diff.
    fn patches(
        &self,
        dir: PathBuf,
        from: String,
        to: String,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<BTreeMap<String, String>>>;
    /// Puts `paths` back as they are in `base` (deleting files `base` lacks).
    fn restore(&self, dir: PathBuf, base: String, paths: Vec<RelPath>)
    -> BoxFuture<'_, Result<()>>;
    /// Writes `paths` as they are in `tree` (a snapshot or look of a
    /// worktree of the same repository: what the person reviewed, not what
    /// is on disk by now) into `into`. A file in `into` that no longer
    /// matches `base` is skipped and returned as a conflict.
    fn apply(
        &self,
        tree: String,
        base: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Vec<String>>>;
    /// Writes `paths` as they are in `tree` (as for `apply`) into `into`
    /// whatever `into` holds now: the person chose the worker's version.
    /// `into` is snapshotted first so this can be undone with `restore`.
    /// Paths that cannot be written safely (a folder, a symlink) are
    /// skipped.
    fn take(
        &self,
        tree: String,
        into: PathBuf,
        paths: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<Taken>>;
    /// Commits exactly `paths` in `dir` with `message`; returns the commit id.
    fn commit(
        &self,
        dir: PathBuf,
        paths: Vec<RelPath>,
        message: String,
    ) -> BoxFuture<'_, Result<String>>;
    /// Removes the worktree at `dir` from the repository at `root`. Its
    /// branch is kept. Refused (`Conflict`, naming them) while it holds
    /// ignored files outside ignored folders, such as a `.env`: those are
    /// in no snapshot and would be lost.
    fn remove(&self, root: PathBuf, dir: PathBuf) -> BoxFuture<'_, Result<()>>;
    /// Lets go of every snapshot of the repository at `root` (its main
    /// checkout and its worktrees) except those in `keep`, so that git can
    /// collect them. The core calls it when reviews no longer need them.
    fn prune_snapshots(&self, root: PathBuf, keep: Vec<String>) -> BoxFuture<'_, Result<()>>;
    /// Deletes a worker's `branch` in the repository at `root`. Refused
    /// (`Conflict`) while it has commits that are neither in `base` nor in
    /// HEAD. A branch that is already gone is fine.
    fn delete_branch(
        &self,
        root: PathBuf,
        branch: String,
        base: String,
    ) -> BoxFuture<'_, Result<()>>;
}

/// What [`Worktrees::git_files`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitFiles {
    /// The hooks folder relative to the top of the working tree, when
    /// `core.hooksPath` points inside it (`.husky`): its files are in the
    /// diff and marked protected.
    pub hooks_in_tree: Option<String>,
    /// A version of each file (its content, mode or link target), by the
    /// name the review shows: `.git/config`, `.git/hooks/pre-commit`, or
    /// the path itself for a hooks folder outside the repository.
    pub files: BTreeMap<String, String>,
}

/// What [`Worktrees::take`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// A snapshot of `into` from just before: `restore` from it undoes.
    pub undo: String,
    /// A snapshot of `into` right after writing, to tell later edits from
    /// the take.
    pub after: String,
    /// Paths left as they were.
    pub skipped: Vec<String>,
}

/// The git panel's operations on a project's main checkout (implemented by
/// `ovra-worktree`, `docs/specs/git-panel.md`). `root` is the project root,
/// absolute; paths are relative to it and already validated. Every call
/// runs the git command line with the person's config, but never a file
/// monitor, an external diff or textconv driver, or a terminal prompt.
pub trait GitService: Send + Sync + Debug {
    /// `None` when `root` is not in a git repository.
    fn status(&self, root: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>>;
    fn stage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>>;
    fn unstage(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>>;
    /// Applies `hunk` (from [`GitService::diff`] of `path`) to the index,
    /// or with `unstage` takes it out; `Conflict` when it no longer applies.
    fn stage_hunk(
        &self,
        root: PathBuf,
        path: RelPath,
        hunk: Hunk,
        unstage: bool,
    ) -> BoxFuture<'_, Result<()>>;
    /// Unstaged changes of `paths` back to the index version; untracked
    /// files deleted. The core snapshots the working tree first.
    fn discard(&self, root: PathBuf, paths: Vec<RelPath>) -> BoxFuture<'_, Result<()>>;
    fn commit(
        &self,
        root: PathBuf,
        message: String,
        amend: bool,
    ) -> BoxFuture<'_, Result<Committed>>;
    fn branches(&self, root: PathBuf) -> BoxFuture<'_, Result<Vec<BranchInfo>>>;
    fn switch(&self, root: PathBuf, branch: String) -> BoxFuture<'_, Result<()>>;
    fn create_branch(&self, root: PathBuf, name: String, switch: bool)
    -> BoxFuture<'_, Result<()>>;
    fn diff(&self, root: PathBuf, path: RelPath, staged: bool) -> BoxFuture<'_, Result<FileDiff>>;
    fn show(&self, root: PathBuf, path: RelPath, rev: GitRev)
    -> BoxFuture<'_, Result<GitFileText>>;
    fn blame(&self, root: PathBuf, path: RelPath) -> BoxFuture<'_, Result<Blame>>;
    fn log(&self, root: PathBuf, limit: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>>;
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

impl Unavailable {
    fn later<T: Send + 'static>(&self) -> BoxFuture<'static, Result<T>> {
        let result = self.err();
        Box::pin(async move { result })
    }
}

impl GitService for Unavailable {
    fn status(&self, _: PathBuf) -> BoxFuture<'_, Result<Option<RepoStatus>>> {
        self.later()
    }
    fn stage(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn unstage(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn stage_hunk(&self, _: PathBuf, _: RelPath, _: Hunk, _: bool) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn discard(&self, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn commit(&self, _: PathBuf, _: String, _: bool) -> BoxFuture<'_, Result<Committed>> {
        self.later()
    }
    fn branches(&self, _: PathBuf) -> BoxFuture<'_, Result<Vec<BranchInfo>>> {
        self.later()
    }
    fn switch(&self, _: PathBuf, _: String) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn create_branch(&self, _: PathBuf, _: String, _: bool) -> BoxFuture<'_, Result<()>> {
        self.later()
    }
    fn diff(&self, _: PathBuf, _: RelPath, _: bool) -> BoxFuture<'_, Result<FileDiff>> {
        self.later()
    }
    fn show(&self, _: PathBuf, _: RelPath, _: GitRev) -> BoxFuture<'_, Result<GitFileText>> {
        self.later()
    }
    fn blame(&self, _: PathBuf, _: RelPath) -> BoxFuture<'_, Result<Blame>> {
        self.later()
    }
    fn log(&self, _: PathBuf, _: u32) -> BoxFuture<'_, Result<Vec<CommitInfo>>> {
        self.later()
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
    fn policy(&self, _: &ProjectId) -> Result<Policy> {
        self.err()
    }
    fn set_policy(&self, _: &ProjectId, _: &Policy) -> Result<()> {
        self.err()
    }
    fn allowlist(&self, _: &ProjectId) -> Result<Allowlist> {
        self.err()
    }
    fn set_allowlist(&self, _: &ProjectId, _: &Allowlist) -> Result<()> {
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

impl AuditLog for Unavailable {
    fn append(&self, _: &ProjectId, _: Option<&SessionId>, _: AuditEvent) -> Result<AuditEntry> {
        self.err()
    }
    fn list(&self, _: &ProjectId, _: Option<u64>, _: usize) -> Result<Vec<AuditEntry>> {
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
        _: String,
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
    fn look(&self, _: PathBuf) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.err() })
    }
    fn git_files(&self, _: PathBuf) -> BoxFuture<'_, Result<GitFiles>> {
        Box::pin(async move { self.err() })
    }
    fn diff(&self, _: PathBuf, _: String, _: String) -> BoxFuture<'_, Result<Vec<Delta>>> {
        Box::pin(async move { self.err() })
    }
    fn patches(
        &self,
        _: PathBuf,
        _: String,
        _: String,
        _: Vec<RelPath>,
    ) -> BoxFuture<'_, Result<BTreeMap<String, String>>> {
        Box::pin(async move { self.err() })
    }
    fn take(&self, _: String, _: PathBuf, _: Vec<RelPath>) -> BoxFuture<'_, Result<Taken>> {
        Box::pin(async move { self.err() })
    }
    fn delete_branch(&self, _: PathBuf, _: String, _: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.err() })
    }
}

impl Secrets for Unavailable {
    fn status(&self) -> SecretStoreStatus {
        SecretStoreStatus::Unavailable {
            reason: format!("{} is not available in this build", self.0),
        }
    }
    fn list(&self, _: &ProjectId) -> Result<Vec<SecretInfo>> {
        Ok(Vec::new())
    }
    fn set(&self, _: &ProjectId, _: SecretInfo, _: SecretValue) -> Result<()> {
        self.err()
    }
    fn remove(&self, _: &ProjectId, _: &str) -> Result<()> {
        self.err()
    }
    fn remove_project(&self, _: &ProjectId) -> Result<()> {
        Ok(())
    }
    fn value(&self, _: &ProjectId, _: &str) -> Result<SecretValue> {
        self.err()
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

impl LanguageServers for Unavailable {
    fn known(&self) -> BoxFuture<'_, Vec<KnownServer>> {
        Box::pin(async { Vec::new() })
    }
    fn sandbox(&self) -> Option<String> {
        None
    }
    fn start(&self, _: ServerSpec, _: ServerHost) -> BoxFuture<'_, Result<StartedServer>> {
        Box::pin(async move { self.err() })
    }
}
