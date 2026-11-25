//! Agent sessions: the registry, their transcripts and the [`AgentHost`]
//! runtimes report through. Runtimes themselves live behind
//! [`AgentRuntime`](crate::services::AgentRuntime).

mod accounts;
mod net;
mod permissions;
mod requests;
mod state;
mod transcript;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::orchestration::direct::Direct;
use crate::protocol::agents::SandboxReport;
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, Billing, SessionId, SessionInfo, SessionMode,
};
use crate::protocol::audit::{AuditEvent, DecidedBy};
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionId, PermissionReason, PermissionRequest, PermissionsEvent,
};
use crate::protocol::policy::PolicyAction;
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    EventSink, IdeTools, NetProxy, ProfileNetwork, RunningAgent, SessionStore, StoredSession,
    ToolCaller, ToolRole, Worktrees,
};

pub(crate) use permissions::Permissions;
pub(crate) use requests::{
    Ctx, NewSession, Parts, agents as handle_agents, check_text, permissions as handle_permissions,
    profile, resume, send, start, start_queued, stop, stop_project, trust,
};
pub(crate) use transcript::MAX_TEXT_BYTES;
use transcript::Transcript;

/// The rule the audit log names when a policy could not be read.
const UNREADABLE_POLICY: &str = "the policy could not be read";

/// Ended sessions kept per project, with their transcripts; older ones are
/// forgotten when a new session starts.
const MAX_ENDED_PER_PROJECT: usize = 20;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
struct Session {
    info: SessionInfo,
    transcript: Transcript,
    process: Option<Arc<dyn RunningAgent>>,
    /// Set by `agents.stop`: the session takes no more messages and its
    /// agent may no longer ask for permissions.
    stopping: bool,
    /// The agent's own id for the conversation, which `agents.resume` needs.
    agent_session: Option<String>,
    /// How this run started it; `None` for a session loaded from the store.
    launch: Option<Launch>,
    /// Whether this run loads the project's own agent settings.
    trusted: bool,
    /// Counts the runs of this session: a resume starts the next one.
    run: u64,
    /// The network of this run's agent: its browser goes through the same
    /// netproxy.
    network: Option<ProfileNetwork>,
    /// The first message of a `queued` session, until it starts.
    queued: Option<Parts>,
    /// The pid of this run's agent process, as the runtime reported it.
    pid: Option<u32>,
}

/// A session that has not ended, as the resource monitor sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Running {
    pub(crate) id: SessionId,
    pub(crate) project: ProjectId,
    pub(crate) mode: SessionMode,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) pid: Option<u32>,
    pub(crate) queued: bool,
}

/// Where a session works and which IDE tools it gets; kept to resume it the
/// same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Launch {
    pub(crate) cwd: PathBuf,
    pub(crate) tools: Option<ToolRole>,
}

/// Every session kept, in start order: this run's and those loaded from the
/// [`SessionStore`]. Store failures do not stop sessions; they are only not
/// kept.
#[derive(Debug)]
pub(crate) struct Sessions {
    list: Mutex<Vec<Session>>,
    store: Arc<dyn SessionStore>,
    started: Mutex<u64>,
    pub(crate) permissions: Permissions,
    ide: Mutex<Option<IdeLink>>,
    /// Direct sessions' baselines and which files are their agents'.
    pub(crate) direct: Direct,
    /// Each running session's network goes through its proxy here; without
    /// one, sessions' network is open.
    net: Option<Arc<dyn NetProxy>>,
    /// Held while a project's allowlist is stored and passed to proxies,
    /// so that running proxies end with the stored list.
    allowlists: Mutex<()>,
}

/// Where sessions' IDE tool calls go; set once when the core is built.
#[derive(Debug)]
struct IdeLink {
    tools: Arc<dyn IdeTools>,
    core: crate::core::WeakCore,
}

impl Sessions {
    /// Loads the kept sessions. Those that ran when the core last quit have
    /// no process any more, so they show as stopped.
    pub(crate) fn new(
        store: Arc<dyn SessionStore>,
        worktrees: Arc<dyn Worktrees>,
        net: Option<Arc<dyn NetProxy>>,
    ) -> Self {
        let mut started = 0;
        let mut list = Vec::new();
        for (stored, messages) in store.load(transcript::MAX_MESSAGES).unwrap_or_default() {
            let number = stored
                .info
                .id
                .0
                .strip_prefix('s')
                .and_then(|n| n.parse().ok());
            started = started.max(number.unwrap_or(0));
            let mut session = Session {
                transcript: Transcript::restore(messages, stored.info.usage.clone()),
                info: stored.info,
                process: None,
                stopping: false,
                agent_session: stored.agent_session,
                launch: None,
                trusted: false,
                run: 0,
                network: None,
                queued: None,
                pid: None,
            };
            if !session.info.status.is_ended() {
                session.info.status = AgentStatus::Stopped;
                let _ = store.save(&session.stored());
            }
            list.push(session);
        }
        Self {
            list: Mutex::new(list),
            store,
            started: Mutex::new(started),
            permissions: Permissions::default(),
            ide: Mutex::new(None),
            direct: Direct::new(worktrees),
            net,
            allowlists: Mutex::new(()),
        }
    }

    pub(crate) fn link_ide(&self, tools: Arc<dyn IdeTools>, core: crate::core::WeakCore) {
        *lock(&self.ide) = Some(IdeLink { tools, core });
    }

    /// The core these sessions belong to, while it exists.
    pub(crate) fn core(&self) -> Option<crate::Core> {
        lock(&self.ide).as_ref()?.core.upgrade()
    }

    pub(crate) fn next_id(&self) -> SessionId {
        let mut started = lock(&self.started);
        *started += 1;
        SessionId(format!("s{started}"))
    }

    /// Adds a session that works in `launch`; `queued`: the first message
    /// of a session that waits for a place.
    pub(crate) fn insert(&self, info: SessionInfo, launch: Launch, queued: Option<Parts>) {
        let mut list = lock(&self.list);
        let project = info.project.clone();
        let session = Session {
            info,
            transcript: Transcript::default(),
            process: None,
            stopping: false,
            agent_session: None,
            launch: Some(launch),
            trusted: false,
            run: 0,
            network: None,
            queued,
            pid: None,
        };
        let _ = self.store.save(&session.stored());
        list.push(session);
        let ended = |s: &Session| s.info.project == project && s.info.status.is_ended();
        let mut extra = list
            .iter()
            .filter(|s| ended(s))
            .count()
            .saturating_sub(MAX_ENDED_PER_PROJECT);
        list.retain(|s| {
            let forget = extra > 0 && ended(s);
            if forget {
                extra -= 1;
                let _ = self.store.remove(&s.info.id);
            }
            !forget
        });
    }

    pub(crate) fn list(&self, project: &ProjectId) -> Vec<SessionInfo> {
        lock(&self.list)
            .iter()
            .filter(|s| &s.info.project == project)
            .map(Session::info)
            .collect()
    }

    pub(crate) fn find<T>(
        &self,
        id: &SessionId,
        f: impl FnOnce(&mut SessionView<'_>) -> T,
    ) -> Result<T, ProtocolError> {
        let mut list = lock(&self.list);
        let session = list.iter_mut().find(|s| &s.info.id == id).ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, format!("session {} not found", id.0))
        })?;
        Ok(f(&mut SessionView {
            session,
            permissions: &self.permissions,
            store: self.store.as_ref(),
        }))
    }

    /// Makes a request taken for "Always allow" pending again, and says so
    /// anew, so a client that reloaded meanwhile still shows it. Under the
    /// sessions' lock: a request of a session that stopped or ended since
    /// it was taken, which the stop could not deny, is denied and resolved
    /// instead.
    pub(crate) fn put_back(&self, taken: permissions::Taken, sink: &EventSink) {
        let request = taken.request.clone();
        let list = lock(&self.list);
        let live = list
            .iter()
            .any(|s| s.info.id == request.session && !s.stopping && !s.info.status.is_ended());
        let pending = self.permissions.put_back(taken, live);
        drop(list);
        if pending {
            sink.emit(PermissionsEvent::Requested(Box::new(request)));
        } else {
            sink.emit(PermissionsEvent::Resolved {
                id: request.id,
                decision: Decision::Deny,
            });
        }
    }

    /// Whether a session that writes the main checkout of `project` runs:
    /// a direct session other than a coordinator, which only reads.
    pub(crate) fn direct_writer(&self, project: &ProjectId) -> bool {
        lock(&self.list).iter().any(|s| {
            !s.info.status.is_ended()
                && &s.info.project == project
                && s.info.mode == SessionMode::Direct
                && s.launch.as_ref().and_then(|l| l.tools) != Some(ToolRole::Coordinator)
        })
    }

    /// Sessions of `project` that have not ended, with their state and
    /// which IDE tools they have.
    pub(crate) fn active(
        &self,
        project: &ProjectId,
    ) -> Vec<(SessionId, AgentStatus, Option<ToolRole>, SessionMode)> {
        lock(&self.list)
            .iter()
            .filter(|s| &s.info.project == project && !s.info.status.is_ended())
            .map(|s| {
                let role = s.launch.as_ref().and_then(|l| l.tools);
                (s.info.id.clone(), s.info.status.clone(), role, s.info.mode)
            })
            .collect()
    }

    /// Every session that has not ended, of every project, for the
    /// resource monitor.
    pub(crate) fn not_ended(&self) -> Vec<Running> {
        lock(&self.list)
            .iter()
            .filter(|s| !s.info.status.is_ended())
            .map(|s| Running {
                id: s.info.id.clone(),
                project: s.info.project.clone(),
                mode: s.info.mode,
                cwd: s.launch.as_ref().map(|l| l.cwd.clone()),
                pid: s.pid,
                queued: s.info.status == AgentStatus::Queued,
            })
            .collect()
    }

    /// Moves the oldest `queued` session of `project` to `starting`, without
    /// telling anyone yet, and hands over its first message.
    pub(crate) fn dequeue(&self, project: &ProjectId) -> Option<(SessionId, Parts)> {
        let mut list = lock(&self.list);
        let session = list
            .iter_mut()
            .find(|s| &s.info.project == project && s.info.status == AgentStatus::Queued)?;
        session.info.status = AgentStatus::Starting;
        let _ = self.store.save(&session.stored());
        Some((
            session.info.id.clone(),
            session.queued.take().unwrap_or_default(),
        ))
    }

    /// Sessions of `project` that have not ended.
    pub(crate) fn running(&self, project: &ProjectId) -> Vec<SessionId> {
        lock(&self.list)
            .iter()
            .filter(|s| &s.info.project == project && !s.info.status.is_ended())
            .map(|s| s.info.id.clone())
            .collect()
    }
}

impl Session {
    fn info(&self) -> SessionInfo {
        SessionInfo {
            usage: self.transcript.usage().clone(),
            resumable: self.info.status.is_ended() && self.agent_session.is_some(),
            ..self.info.clone()
        }
    }

    fn stored(&self) -> StoredSession {
        StoredSession {
            info: self.info(),
            agent_session: self.agent_session.clone(),
        }
    }
}

/// Access to one session while the registry is locked. Lock order: the
/// registry, then the permission requests or the store.
pub(crate) struct SessionView<'a> {
    session: &'a mut Session,
    permissions: &'a Permissions,
    store: &'a dyn SessionStore,
}

impl SessionView<'_> {
    pub(crate) fn info(&self) -> SessionInfo {
        self.session.info()
    }
    pub(crate) fn status(&self) -> &AgentStatus {
        &self.session.info.status
    }
    pub(crate) fn transcript(&self) -> Vec<AgentMessage> {
        self.session.transcript.messages()
    }
    /// Adds a message and returns it as kept (long texts cut).
    pub(crate) fn push(&mut self, message: AgentMessage) -> AgentMessage {
        let kept = self.session.transcript.push(message);
        let _ = self.store.append(&self.session.info.id, &kept);
        kept
    }
    fn save(&self) {
        let _ = self.store.save(&self.session.stored());
    }
    pub(crate) fn agent_session(&self) -> Option<String> {
        self.session.agent_session.clone()
    }
    pub(crate) fn launch(&self) -> Option<Launch> {
        self.session.launch.clone()
    }
    pub(crate) fn set_launch(&mut self, launch: Launch) {
        self.session.launch = Some(launch);
    }
    pub(crate) fn network(&self) -> Option<ProfileNetwork> {
        self.session.network.clone()
    }
    fn set_network(&mut self, network: ProfileNetwork) {
        self.session.network = Some(network);
    }
    fn set_trusted(&mut self, trusted: bool) {
        self.session.trusted = trusted;
    }
    fn set_agent_session(&mut self, id: String) {
        if self.session.agent_session.as_ref() != Some(&id) {
            self.session.agent_session = Some(id);
            self.save();
        }
    }
    /// Records how the current process runs; true if that changed.
    fn set_sandbox(&mut self, report: Option<SandboxReport>) -> bool {
        let info = &mut self.session.info;
        if info.sandbox_reported && info.sandbox == report {
            return false;
        }
        info.sandbox = report;
        info.sandbox_reported = true;
        self.save();
        true
    }
    /// Records who pays; true if that changed.
    fn set_billing(&mut self, billing: Billing) -> bool {
        if self.session.info.billing.as_ref() == Some(&billing) {
            return false;
        }
        self.session.info.billing = Some(billing);
        self.save();
        true
    }
    pub(crate) fn run(&self) -> u64 {
        self.session.run
    }
    /// Starts an ended session over for `agents.resume`.
    pub(crate) fn restart(&mut self) {
        self.session.run += 1;
        self.session.info.status = AgentStatus::Starting;
        self.session.stopping = false;
        // The new process reports its own sandbox; until it does, none.
        self.session.info.sandbox = None;
        self.session.info.sandbox_reported = false;
        self.save();
    }
    /// Moves to `status` if the state machine allows it; `false` if not.
    fn set_status(&mut self, status: AgentStatus) -> bool {
        let current = &self.session.info.status;
        let held = !status.is_ended()
            && status != AgentStatus::WaitingForApproval
            && self.permissions.has_pending(&self.session.info.id);
        if *current == status || held || !state::allowed(current, &status) {
            return false;
        }
        if status.is_ended() {
            self.session.process = None;
            self.session.pid = None;
        }
        self.session.info.status = status;
        self.save();
        true
    }
    pub(crate) fn process(&self) -> Option<Arc<dyn RunningAgent>> {
        self.session.process.clone()
    }
    pub(crate) fn set_process(&mut self, process: Arc<dyn RunningAgent>) {
        if !self.session.info.status.is_ended() {
            self.session.process = Some(process);
        }
    }
    pub(crate) fn stopping(&self) -> bool {
        self.session.stopping
    }
    /// Marks the session as stopping and denies what its agent waits for.
    pub(crate) fn begin_stop(&mut self) -> Vec<PermissionId> {
        self.session.stopping = true;
        self.permissions.deny_session(
            &self.session.info.id,
            &PermissionAnswer::deny("the session was stopped"),
            &DecidedBy::SessionEnded,
        )
    }
}

/// How a runtime reports one session to the core. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AgentHost {
    session: SessionId,
    project: ProjectId,
    sessions: Arc<Sessions>,
    sink: EventSink,
}

impl AgentHost {
    /// Passes one MCP JSON-RPC message from the session to Ovra's IDE tools
    /// and returns the reply (`None` for a notification). `role` is the
    /// session's [`AgentSpec::tools`](crate::services::AgentSpec::tools).
    pub async fn call_ide(
        &self,
        role: ToolRole,
        message: serde_json::Value,
    ) -> Option<serde_json::Value> {
        let (tools, core) = {
            let ide = lock(&self.sessions.ide);
            let link = ide.as_ref()?;
            (link.tools.clone(), link.core.upgrade()?)
        };
        let caller = ToolCaller {
            session: self.session.clone(),
            project: self.project.clone(),
            role,
        };
        tools.handle(core, caller, message).await
    }

    pub(crate) fn new(
        session: SessionId,
        project: ProjectId,
        sessions: Arc<Sessions>,
        sink: EventSink,
    ) -> Self {
        Self {
            session,
            project,
            sessions,
            sink,
        }
    }

    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// Records the agent's own id for the conversation, so that the session
    /// can be resumed later, also after a restart.
    pub fn agent_session(&self, id: String) {
        let _ = self
            .sessions
            .find(&self.session, |s| s.set_agent_session(id));
    }

    /// The runtime started the agent's process as `pid` (the sandbox's own
    /// process when it wraps the agent), so that its memory and CPU can be
    /// counted with every command it runs. Reported after every spawn.
    pub fn process(&self, pid: u32) {
        let _ = self.sessions.find(&self.session, |s| {
            if !s.session.info.status.is_ended() {
                s.session.pid = Some(pid);
            }
        });
        if let Some(core) = self.sessions.core() {
            crate::resources::sample_now(&core);
        }
    }

    /// The agent said who pays for the session's usage.
    pub fn billing(&self, billing: Billing) {
        let changed = self
            .sessions
            .find(&self.session, |s| s.set_billing(billing.clone()));
        if changed == Ok(true) {
            self.sink.emit(AgentsEvent::Billing {
                session: self.session.clone(),
                billing,
            });
        }
    }

    /// What the project's policy says about a call of this session, in its
    /// folder and sandbox as the runtime reported them. `Err` when the
    /// policy or the session cannot be read: callers then never skip a
    /// question, and the hook refuses.
    fn policy(
        &self,
        tool: &str,
        input: &serde_json::Value,
    ) -> Result<Option<(PolicyAction, String)>, ()> {
        let core = self.sessions.core().ok_or(())?;
        let (cwd, sandbox) = self
            .sessions
            .find(&self.session, |s| {
                (s.launch().map(|l| l.cwd), s.info().sandbox)
            })
            .map_err(|_| ())?;
        let policy = core.project_store().policy(&self.project).map_err(|_| ())?;
        Ok(crate::policy::decide(
            &policy,
            tool,
            input,
            &cwd.ok_or(())?,
            sandbox.as_ref(),
        ))
    }

    /// The runtime ran the session as `report` says (`None`: without a
    /// sandbox). Runtimes report after every spawn; one that never does
    /// counts as unsandboxed.
    ///
    /// Each report is one run of the agent, so it is what the audit log
    /// records as the session's start.
    pub fn sandboxed(&self, report: Option<SandboxReport>) {
        let started = self.sessions.find(&self.session, |s| {
            let changed = s.set_sandbox(report.clone());
            let event = AuditEvent::SessionStarted {
                mode: s.session.info.mode,
                trusted: s.session.trusted,
                sandbox: report.clone(),
            };
            (changed, event)
        });
        if let Ok((changed, event)) = started {
            if changed {
                self.sink.emit(AgentsEvent::Sandbox {
                    session: self.session.clone(),
                    sandbox: report,
                });
            }
            self.audit(event);
        }
    }

    fn audit(&self, event: AuditEvent) {
        if let Some(core) = self.sessions.core() {
            crate::audit::record(&core, &self.project, Some(&self.session), event);
        }
    }

    /// Records who decided a tool call. The entry is queued before this
    /// returns, so it is kept even if the caller stops waiting for it; the
    /// returned future waits until it is written.
    fn audit_tool(
        &self,
        tool: &str,
        input: &serde_json::Value,
        allowed: bool,
        by: DecidedBy,
    ) -> impl Future<Output = ()> + use<> {
        let recorded = self.sessions.core().map(|core| {
            crate::audit::tool_decided(
                &core,
                &self.project,
                Some(&self.session),
                tool,
                input,
                allowed,
                by,
            )
        });
        async move {
            if let Some(recorded) = recorded {
                let _ = recorded.await;
            }
        }
    }

    /// Records who decided a tool call and passes the answer on.
    async fn decided(
        &self,
        tool: &str,
        input: &serde_json::Value,
        answer: PermissionAnswer,
        by: DecidedBy,
    ) -> PermissionAnswer {
        let allowed = answer.decision == Decision::Allow;
        self.audit_tool(tool, input, allowed, by).await;
        answer
    }

    /// The agent is about to run a tool: the input of a `PreToolUse` hook
    /// (`tool_use_id`, `tool_name`, `tool_input`). The runtime awaits this
    /// before the tool runs, so a direct session can snapshot the tree
    /// first (ADR 0005).
    ///
    /// Returns what the project's policy says about the call: the hook also
    /// sees calls the user's own settings allow without asking, which never
    /// reach [`Self::ask`] unless the hook tells the CLI to ask.
    pub async fn before_tool(&self, hook_input: &serde_json::Value) -> BeforeTool {
        let field = |name: &str| hook_input.get(name).and_then(serde_json::Value::as_str);
        // The policy decides whenever the tool is known; only the
        // bookkeeping below needs the call's id.
        let Some(name) = field("tool_name") else {
            return BeforeTool::Proceed;
        };
        let input = hook_input
            .get("tool_input")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(why) = self.outside_research(name) {
            self.audit_tool(name, &input, false, DecidedBy::Role).await;
            return BeforeTool::Deny(why);
        }
        // Ovra's own tools follow their role rules (`Self::ask`).
        let mut verdict = BeforeTool::Proceed;
        if !name.starts_with(crate::orchestration::IDE_TOOL_PREFIX) {
            let refused = match self.policy(name, &input) {
                Ok(Some((PolicyAction::Deny, rule))) => Some((
                    format!(
                        "The project's policy refuses this ({rule}). Ask the person if it is needed."
                    ),
                    rule,
                )),
                Err(()) => Some((
                    "Ovra could not read the project's policy, so it refuses tool calls."
                        .to_owned(),
                    UNREADABLE_POLICY.to_owned(),
                )),
                // The CLI asks through `can_use_tool`, and `Self::ask`
                // asks the person, whatever the user's settings allow.
                Ok(Some((PolicyAction::Ask, rule))) => {
                    verdict = BeforeTool::Ask(format!(
                        "The project's policy asks the person first ({rule})."
                    ));
                    None
                }
                // Auto never skips more than the user's settings: the CLI
                // decides as usual, and `Self::ask` applies the rule.
                Ok(Some((PolicyAction::Auto, _)) | None) => None,
            };
            if let Some((why, rule)) = refused {
                let by = DecidedBy::Policy { rule };
                self.audit_tool(name, &input, false, by).await;
                return BeforeTool::Deny(why);
            }
        }
        if let Some(id) = field("tool_use_id") {
            self.sessions
                .direct
                .tool_started(&self.session, id, name, &input)
                .await;
        }
        verdict
    }

    /// Finishes the bookkeeping for tool results reported with
    /// [`Self::message`]. Runtimes await it after passing on a result, so a
    /// direct session snapshots the tree right when the tool is done.
    pub async fn tools_settled(&self) {
        self.sessions.direct.catch_up(&self.session).await;
    }

    /// Adds a message to the transcript and shows it.
    pub fn message(&self, message: AgentMessage) {
        if let AgentMessage::ToolResult { tool_use_id, .. } = &message {
            self.sessions.direct.tool_ended(&self.session, tool_use_id);
        }
        if let Ok(message) = self.sessions.find(&self.session, |s| s.push(message)) {
            self.sink.emit(AgentsEvent::Message {
                session: self.session.clone(),
                message,
            });
        }
    }

    /// Shows the next piece of the assistant text the agent is writing. Not
    /// kept: the complete message comes through [`Self::message`].
    pub fn text_delta(&self, text: String) {
        self.sink.emit(AgentsEvent::TextDelta {
            session: self.session.clone(),
            text,
        });
    }

    /// Records a new state, if the state machine allows it. An ended session
    /// drops its pending permission requests, which the agent sees as denied.
    pub fn status(&self, status: AgentStatus) {
        let ended = status.is_ended();
        if ended {
            self.sessions.direct.session_ended(&self.session);
        }
        if self.set_status(status) && ended {
            let dropped = self.sessions.permissions.drop_session(&self.session);
            self.resolved(dropped, Decision::Deny);
        }
    }

    pub(crate) fn set_status(&self, status: AgentStatus) -> bool {
        let changed = self.sessions.find(&self.session, |s| {
            let changed = s.set_status(status.clone());
            // Under the sessions' lock, so a resume cannot start a new
            // proxy in between.
            if changed && status.is_ended() {
                self.sessions.net_stop(&self.session);
            }
            (changed, s.run())
        });
        // The run that ended: a resume may have started the next one since.
        let (changed, run) = changed.unwrap_or((false, 0));
        if changed {
            self.sink.emit(AgentsEvent::Status {
                session: self.session.clone(),
                project: self.project.clone(),
                status: status.clone(),
            });
            if let Some(core) = self.sessions.core() {
                crate::resources::wake(&core);
                crate::orchestration::on_status(&core, &self.session, &status);
                if status.is_ended() {
                    crate::servers::session_ended(&core, &self.session, run);
                    crate::browser::session_ended(&core, &self.session);
                }
            }
        }
        changed
    }

    pub(crate) fn resolved(&self, ids: Vec<PermissionId>, decision: Decision) {
        for id in ids {
            self.sink.emit(PermissionsEvent::Resolved { id, decision });
        }
    }

    /// Asks a person whether the agent may use `tool` with `input`, and
    /// waits for the answer. The session shows as waiting meanwhile. A
    /// stopping or ended session is denied without asking. Dropping the
    /// future withdraws the request.
    pub async fn ask(
        &self,
        tool: String,
        input: serde_json::Value,
        title: Option<String>,
        reason: Option<PermissionReason>,
    ) -> PermissionAnswer {
        if let Some(why) = self.outside_research(&tool) {
            let answer = PermissionAnswer::deny(why);
            return self.decided(&tool, &input, answer, DecidedBy::Role).await;
        }
        if let Some(name) = tool.strip_prefix(crate::orchestration::IDE_TOOL_PREFIX) {
            // Ovra's own tools are decided by role and limits, never asked.
            return match self.sessions.core() {
                Some(core) => {
                    let answer = crate::orchestration::decide(&core, &self.session, name, &input);
                    self.decided(&tool, &input, answer, DecidedBy::Role).await
                }
                None => PermissionAnswer::deny("Ovra is shutting down"),
            };
        }
        let decided = self.policy(&tool, &input);
        // A call some rule asks about, or under an unreadable policy, gets
        // no "Always allow".
        let open = matches!(decided, Ok(None));
        match decided {
            Ok(Some((PolicyAction::Deny, rule))) => {
                // The hook may have seen the call first (a policy changed
                // between the two).
                self.sessions
                    .direct
                    .tool_answered(&self.session, &tool, &input, false)
                    .await;
                let answer = PermissionAnswer::deny(format!(
                    "The project's policy refuses this ({rule}). Ask the person if it is needed."
                ));
                return self
                    .decided(&tool, &input, answer, DecidedBy::Policy { rule })
                    .await;
            }
            Ok(Some((PolicyAction::Auto, rule))) => {
                let by = DecidedBy::Policy { rule };
                return self
                    .decided(&tool, &input, PermissionAnswer::allow(), by)
                    .await;
            }
            // A research session's web search runs on Anthropic's side and
            // touches nothing here (ADR 0013).
            Ok(None) if tool == WEB_SEARCH && self.is_research() => {
                return self
                    .decided(&tool, &input, PermissionAnswer::allow(), DecidedBy::Role)
                    .await;
            }
            // Unreadable policy: ask, never decide on its own.
            _ => {}
        }
        let tool_input = (tool.clone(), input.clone());
        let registered = self.sessions.find(&self.session, |s| {
            if s.stopping() {
                return Err(PermissionAnswer::deny("the session was stopped"));
            }
            if s.status().is_ended() {
                return Err(PermissionAnswer::deny("the session ended"));
            }
            let cwd = s.launch().map(|l| l.cwd);
            let sandbox = s.info().sandbox;
            let always = cwd
                .as_deref()
                .filter(|_| open)
                .and_then(|cwd| crate::policy::always_rule(&tool, &input, cwd, sandbox.as_ref()));
            Ok(s.permissions.register(PermissionRequest {
                id: PermissionId(String::new()),
                session: self.session.clone(),
                project: s.info().project,
                cwd: cwd.map(|c| c.to_string_lossy().into_owned()),
                tool,
                input,
                title,
                reason,
                always,
            }))
        });
        let (request, answer) = match registered {
            Ok(Ok(registered)) => registered,
            Ok(Err(answer)) => {
                let (tool, input) = &tool_input;
                return self
                    .decided(tool, input, answer, DecidedBy::SessionEnded)
                    .await;
            }
            Err(_) => {
                let (tool, input) = &tool_input;
                let answer = PermissionAnswer::deny("the session ended");
                return self
                    .decided(tool, input, answer, DecidedBy::SessionEnded)
                    .await;
            }
        };
        let mut waiting = Waiting {
            host: self,
            id: Some(request.id.clone()),
            settled: false,
        };
        let (tool, input) = tool_input;
        let direct = &self.sessions.direct;
        direct.tool_waiting(&self.session, &tool, &input).await;
        self.sink
            .emit(PermissionsEvent::Requested(Box::new(request)));
        self.set_status(AgentStatus::WaitingForApproval);
        let (answer, by) = answer.await.unwrap_or_else(|_| {
            (
                PermissionAnswer::deny("the session ended"),
                DecidedBy::SessionEnded,
            )
        });
        waiting.id = None;
        // Queued at once, so the entry survives the future being dropped.
        let allowed = answer.decision == Decision::Allow;
        let recorded = self.audit_tool(&tool, &input, allowed, by);
        direct
            .tool_answered(&self.session, &tool, &input, allowed)
            .await;
        // Ignored while other requests of the session still wait.
        self.set_status(AgentStatus::Working);
        waiting.settled = true;
        recorded.await;
        answer
    }
}

/// The only tools a research session may use (ADR 0013); the runtime offers
/// no others, and the core refuses any other one it is asked about.
pub(crate) const RESEARCH_TOOLS: [&str; 4] = ["Read", "Grep", "Glob", WEB_SEARCH];
const WEB_SEARCH: &str = "WebSearch";

impl AgentHost {
    fn is_research(&self) -> bool {
        // A session that is gone is no research session.
        self.sessions
            .find(&self.session, |s| s.info().mode == SessionMode::Research)
            .unwrap_or(false)
    }

    /// Why `tool` is refused, if this is a research session and the tool is
    /// not one of [`RESEARCH_TOOLS`].
    fn outside_research(&self, tool: &str) -> Option<String> {
        let Ok(research) = self
            .sessions
            .find(&self.session, |s| s.info().mode == SessionMode::Research)
        else {
            // Fails closed: a session that is gone runs nothing.
            return Some("The session has ended.".to_owned());
        };
        (research && !RESEARCH_TOOLS.contains(&tool)).then(|| {
            format!(
                "A research session only reads the project and searches the web; {tool} is \
                 not available. Put what should change in your report instead."
            )
        })
    }
}

/// What the project's policy says about a tool call before it runs
/// ([`AgentHost::before_tool`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a deny or ask from the project's policy must reach the CLI"]
pub enum BeforeTool {
    /// No deny or ask rule matches: the CLI decides as usual.
    Proceed,
    /// The person is asked, even if the user's settings allow the call.
    Ask(String),
    /// Refused, for this reason.
    Deny(String),
}

/// Withdraws the request when the agent stops waiting before it is answered
/// (the [`AgentHost::ask`] future is dropped, e.g. the agent cancelled it),
/// and shows the session working again if it stops after the answer but
/// before that.
struct Waiting<'a> {
    host: &'a AgentHost,
    id: Option<PermissionId>,
    settled: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        match self.id.take() {
            Some(id) => {
                if self.host.sessions.permissions.withdraw(&id) {
                    self.host.resolved(vec![id], Decision::Deny);
                    self.host.set_status(AgentStatus::Working);
                }
            }
            None if !self.settled => {
                self.host.set_status(AgentStatus::Working);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
