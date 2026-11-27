//! Agent sessions: the registry, their transcripts and the [`AgentHost`]
//! runtimes report through. Runtimes themselves live behind
//! [`AgentRuntime`](crate::services::AgentRuntime).

mod accounts;
mod history;
mod keys;
mod net;
mod permissions;
mod plan;
mod queue;
mod requests;
mod state;
mod stuck;
mod transcript;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::orchestration::direct::Direct;
use crate::protocol::agents::SandboxReport;
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, Billing, SessionId, SessionInfo, SessionMode, ToolCall,
    ToolKind,
};
use crate::protocol::audit::{AuditEvent, DecidedBy};
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionId, PermissionReason, PermissionRequest, PermissionsEvent,
};
use crate::protocol::policy::{PolicyAction, PolicyRule};
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::SecretInfo;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    AgentImage, AgentRuntime, EventSink, IdeTools, NetProxy, ProfileNetwork, RunningAgent,
    SessionStore, StoredSession, ToolCaller, ToolRole, Worktrees,
};

pub(crate) use permissions::Permissions;
pub(crate) use requests::{
    Ctx, NewSession, Parts, agents as handle_agents, can_plan_first, check_model, check_text,
    permissions as handle_permissions, profile, resume, sees_images, send, start, start_queued,
    stop, stop_project, trust,
};
pub(crate) use transcript::MAX_TEXT_BYTES;
use transcript::Transcript;

/// The rule the audit log names when a policy could not be read.
const UNREADABLE_POLICY: &str = "the policy could not be read";

/// Ended sessions kept per project, with their transcripts; older ones are
/// forgotten when a new session starts. History reaches back this far.
const MAX_ENDED_PER_PROJECT: usize = 50;

/// Unix time in milliseconds; 0 for a clock before 1970.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Held while the agent settings are read, changed and written back, so
/// that writers (requests, limits, the schedules' clock) never lose each
/// other's changes. Nothing written if `f` fails.
pub(crate) fn change_settings<T>(
    store: &dyn crate::services::ProjectStore,
    f: impl FnOnce(&mut crate::protocol::agents::AgentSettings) -> Result<T, ProtocolError>,
) -> Result<T, ProtocolError> {
    static WRITING: Mutex<()> = Mutex::new(());
    let _writing = lock(&WRITING);
    let mut settings = store.agent_settings()?;
    let out = f(&mut settings)?;
    store.set_agent_settings(settings)?;
    Ok(out)
}

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
    /// The first message of a `queued` or `preparing` session, until it
    /// starts.
    queued: Option<Parts>,
    /// The pid of this run's agent process, as the runtime reported it.
    pid: Option<u32>,
    /// The images of its first message, until its agent starts.
    images: Vec<AgentImage>,
    /// The person's answers about keys, by name, for this session's runs
    /// (`docs/specs/secrets-consent.md`): `true` allowed.
    keys: std::collections::HashMap<SecretInfo, bool>,
    /// What the person queued for after the running turn.
    later: queue::Later,
    /// Whether its current turn looks stuck.
    watch: stuck::Watch,
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
    /// One question at a time per session and key: requests that would
    /// carry the key meanwhile wait for its answer.
    asking_keys: keys::Asking,
    /// Whether a loop looks for silent sessions ([`stuck::wake`]).
    silence_watch: Mutex<bool>,
}

/// Where sessions' IDE tool calls go; set once when the core is built.
#[derive(Debug)]
struct IdeLink {
    tools: Arc<dyn IdeTools>,
    core: crate::core::WeakCore,
}

impl Sessions {
    /// Loads the kept sessions. Those that ran when the core last quit have
    /// no process any more, so they show as stopped. Tool calls saved
    /// before transcripts carried their shared terms get them from
    /// `runtime`.
    pub(crate) fn new(
        store: Arc<dyn SessionStore>,
        worktrees: Arc<dyn Worktrees>,
        net: Option<Arc<dyn NetProxy>>,
        runtime: &dyn AgentRuntime,
    ) -> Self {
        let mut started = 0;
        let mut list = Vec::new();
        let direct = Direct::new(worktrees, store.clone());
        for (stored, messages) in store.load(transcript::MAX_MESSAGES).unwrap_or_default() {
            let number = stored
                .info
                .id
                .0
                .strip_prefix('s')
                .and_then(|n| n.parse().ok());
            started = started.max(number.unwrap_or(0));
            let agent = stored.info.agent;
            let messages = messages
                .into_iter()
                .map(|m| {
                    transcript::with_call(m, |name, input| runtime.tool_call(agent, name, input))
                })
                .collect();
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
                images: Vec::new(),
                keys: Default::default(),
                later: queue::Later::default(),
                watch: stuck::Watch::default(),
            };
            // What a go-back restored is kept with the checkpoints, if it
            // still can be undone.
            session.info.can_undo_go_back = direct.can_undo_go_back(&session.info.id);
            session.info.stuck = None;
            if !session.info.status.is_ended() {
                // Cut off in the middle of its work: it can go on from there.
                session.info.interrupted = matches!(
                    session.info.status,
                    AgentStatus::Starting | AgentStatus::Working | AgentStatus::WaitingForApproval
                ) && session.agent_session.is_some();
                session.info.status = AgentStatus::Stopped;
                let _ = store.save(&session.stored());
            }
            list.push(session);
        }
        direct.keep_only(|id| {
            list.iter()
                .any(|s: &Session| &s.info.id == id && s.info.mode == SessionMode::Direct)
        });
        Self {
            list: Mutex::new(list),
            store,
            started: Mutex::new(started),
            permissions: Permissions::default(),
            ide: Mutex::new(None),
            direct,
            net,
            allowlists: Mutex::new(()),
            asking_keys: Mutex::default(),
            silence_watch: Mutex::new(false),
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
    /// of a session that waits for a place; `images`: its first message's.
    pub(crate) fn insert(
        &self,
        info: SessionInfo,
        launch: Launch,
        queued: Option<Parts>,
        images: Vec<AgentImage>,
    ) {
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
            images,
            keys: Default::default(),
            later: queue::Later::default(),
            watch: stuck::Watch::default(),
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
        drop(list);
        if let Some(core) = self.core() {
            crate::power::sessions_changed(&core);
        }
    }

    /// `agents.history` over the kept sessions of the projects in `names`.
    pub(crate) fn history(
        &self,
        query: &str,
        project: Option<&ProjectId>,
        names: &std::collections::BTreeMap<ProjectId, String>,
    ) -> Vec<crate::protocol::agents::HistoryEntry> {
        let list = lock(&self.list);
        let messages: Vec<Vec<AgentMessage>> =
            list.iter().map(|s| s.transcript.messages()).collect();
        let candidates = list
            .iter()
            .zip(&messages)
            .map(|(s, m)| history::Candidate {
                info: s.info(),
                messages: m.iter().collect(),
            })
            .collect();
        drop(list);
        history::search(candidates, query, project, names)
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

    /// Whether a session that writes the main checkout of `project` runs or
    /// waits to: a direct session other than a coordinator, which only
    /// reads, and other than `besides`.
    pub(crate) fn direct_writer(&self, project: &ProjectId, besides: Option<&SessionId>) -> bool {
        lock(&self.list).iter().any(|s| {
            !s.info.status.is_ended()
                && Some(&s.info.id) != besides
                && &s.info.project == project
                && s.info.mode == SessionMode::Direct
                && s.launch.as_ref().and_then(|l| l.tools) != Some(ToolRole::Coordinator)
        })
    }

    /// Whether any session of any project is queued, starting, working or
    /// waiting for a person: what keeps the computer awake.
    pub(crate) fn any_working(&self) -> bool {
        lock(&self.list)
            .iter()
            .any(|s| !s.info.status.is_ended() && s.info.status != AgentStatus::Idle)
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

    /// Moves the session `id` between `queued` and `preparing` (`to`),
    /// keeping its first message, without telling anyone yet. False if it
    /// is in neither state.
    pub(crate) fn wait(&self, id: &SessionId, to: AgentStatus) -> bool {
        let mut list = lock(&self.list);
        let Some(session) = list.iter_mut().find(|s| &s.info.id == id) else {
            return false;
        };
        let waiting = |s: &AgentStatus| matches!(s, AgentStatus::Queued | AgentStatus::Preparing);
        if !waiting(&session.info.status) || !waiting(&to) {
            return false;
        }
        session.info.status = to;
        let _ = self.store.save(&session.stored());
        true
    }

    /// Moves the `queued` or `preparing` session `id` to `starting`,
    /// without telling anyone yet, and hands over its first message.
    pub(crate) fn take_waiting(&self, id: &SessionId) -> Option<Parts> {
        let mut list = lock(&self.list);
        let session = list.iter_mut().find(|s| {
            &s.info.id == id
                && matches!(s.info.status, AgentStatus::Queued | AgentStatus::Preparing)
        })?;
        session.info.status = AgentStatus::Starting;
        let _ = self.store.save(&session.stored());
        Some(session.queued.take().unwrap_or_default())
    }

    /// Soon takes the snapshots of the turn edges `id` reported
    /// ([`Direct::turn_started`]), so that they show the tree as it was
    /// then. Needs no runtime: the next look at the session takes them.
    pub(crate) fn record_turns(self: &Arc<Self>, id: &SessionId) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let (sessions, id) = (self.clone(), id.clone());
            runtime.spawn(async move { sessions.direct.catch_up(&id).await });
        }
    }

    /// Marks working sessions that showed nothing for [`stuck::SILENT`]:
    /// answers those whose hint changed, and when to look again (`None`:
    /// no session works).
    pub(crate) fn silence(
        &self,
        now: tokio::time::Instant,
    ) -> (
        Vec<(SessionId, Option<crate::protocol::agents::Stuck>)>,
        Option<tokio::time::Instant>,
    ) {
        let mut changed = Vec::new();
        let mut next: Option<tokio::time::Instant> = None;
        let mut working = false;
        for s in lock(&self.list).iter_mut() {
            if s.info.status != AgentStatus::Working {
                continue;
            }
            working = true;
            if let Some(due) = s.watch.silence(now) {
                next = Some(next.map_or(due, |n| n.min(due)));
            }
            if let Some(stuck) = s.watch.changed() {
                changed.push((s.info.id.clone(), stuck));
            }
        }
        // A silent session still works: look again when it may have
        // spoken, so the watch stops once nothing works.
        if working && next.is_none() {
            next = Some(now + stuck::SILENT);
        }
        (changed, next)
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
        let ended = self.info.status.is_ended();
        SessionInfo {
            usage: self.transcript.usage().clone(),
            resumable: ended && self.agent_session.is_some(),
            queue: self.later.shown(),
            stuck: if ended {
                None
            } else {
                self.watch.hint().map(Box::new)
            },
            ..self.info.clone()
        }
    }

    fn stored(&self) -> StoredSession {
        StoredSession {
            // The queue is not kept across restarts.
            info: SessionInfo {
                queue: Vec::new(),
                ..self.info()
            },
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
    /// The number of the last message in [`Self::transcript`].
    pub(crate) fn seq(&self) -> u64 {
        self.session.transcript.seq()
    }
    /// Adds a message and returns it as kept (long texts cut), with its number.
    pub(crate) fn push(&mut self, message: AgentMessage) -> (AgentMessage, u64) {
        let kept = self.session.transcript.push(message);
        self.session.watch.see(&kept);
        // Kept with the session's next save: not a write per message.
        self.session.info.active_ms = Some(now_ms());
        let _ = self.store.append(&self.session.info.id, &kept);
        (kept, self.session.transcript.seq())
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
    /// The person's answer about `key` in this session, if any: for this
    /// binding, so a key rebound since asks again.
    pub(crate) fn key_answer(&self, key: &SecretInfo) -> Option<bool> {
        self.session.keys.get(key).copied()
    }
    pub(crate) fn set_key_answer(&mut self, key: &SecretInfo, allowed: bool) {
        self.session.keys.insert(key.clone(), allowed);
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
    pub(crate) fn take_images(&mut self) -> Vec<AgentImage> {
        std::mem::take(&mut self.session.images)
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
    /// Enters or leaves plan first (`docs/specs/plan-first.md`).
    fn set_planning(&mut self, planning: bool) {
        self.session.info.planning = planning;
        self.save();
    }
    /// The next message to the agent is in the transcript: answers the
    /// number of its turn. A go-back can no longer be undone.
    fn turn_started(&mut self) -> u32 {
        if std::mem::take(&mut self.session.info.can_undo_go_back) {
            self.save();
        }
        self.session.transcript.turns()
    }
    /// Files were set back to before turn `undone_from` (`None`: put back).
    fn went_back(&mut self, undone_from: Option<u32>, can_undo: bool) {
        self.session.info.undone_from = undone_from;
        self.session.info.can_undo_go_back = can_undo;
        self.save();
    }
    /// Starts an ended session over for `agents.resume`.
    pub(crate) fn restart(&mut self) {
        self.session.run += 1;
        self.session.info.status = AgentStatus::Starting;
        self.session.info.active_ms = Some(now_ms());
        self.session.info.interrupted = false;
        self.session.watch.status(&AgentStatus::Starting);
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
        self.session.watch.status(&status);
        self.session.info.status = status;
        self.session.info.active_ms = Some(now_ms());
        self.save();
        true
    }
    /// The stuck hint, if it changed since clients were last told.
    fn stuck_changed(&mut self) -> Option<Option<crate::protocol::agents::Stuck>> {
        self.session.watch.changed()
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

    /// The session listed the tools of the project's own MCP servers
    /// (`mcp__<server>__<tool>`), so the person can turn one off.
    pub fn server_tools(&self, tools: &[String]) {
        if let Some(core) = self.sessions.core() {
            crate::catalog::servers::tools_listed(&core, &self.project, tools);
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
    /// folder and sandbox as the runtime reported them, quiet mode included. `Err` when the
    /// policy or the session cannot be read: callers then never skip a
    /// question, and the hook refuses.
    fn policy(&self, tool: &ToolRequest) -> Result<Option<(PolicyAction, String)>, ()> {
        let core = self.sessions.core().ok_or(())?;
        let (cwd, sandbox, mode) = self
            .sessions
            .find(&self.session, |s| {
                let info = s.info();
                (s.launch().map(|l| l.cwd), info.sandbox, info.mode)
            })
            .map_err(|_| ())?;
        let policy = core.project_store().policy(&self.project).map_err(|_| ())?;
        let cwd = cwd.ok_or(())?;
        // A worker across repositories: each linked copy's own rules too.
        let linked = crate::orchestration::linked_projects(&core, &self.session);
        let across = !linked.is_empty();
        let decided = if linked.is_empty() {
            crate::policy::decide(&policy, &tool.name, &tool.call, &cwd, sandbox.as_ref())
        } else {
            let mut policies = Vec::new();
            for (project, dir) in linked {
                policies.push((core.project_store().policy(&project).map_err(|_| ())?, dir));
            }
            crate::policy::decide_across(
                (&policy, &cwd),
                &policies,
                &tool.name,
                &tool.call,
                sandbox.as_ref(),
            )
        };
        // Quiet mode decides only what no rule does, as an auto rule would
        // (ADR 0015). Not across repositories: its checks know only the
        // main copy, and a command could reach a linked one past that
        // project's file rules (`docs/specs/multi-repo.md`).
        let quiet = decided.is_none()
            && !across
            && crate::policy::quiet::allows(
                &policy,
                &tool.name,
                &tool.call,
                &cwd,
                sandbox.as_ref(),
                mode,
            );
        if quiet {
            return Ok(Some((
                PolicyAction::Auto,
                crate::policy::quiet::QUIET_RULE.to_owned(),
            )));
        }
        Ok(decided)
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
        tool: &ToolRequest,
        allowed: bool,
        by: DecidedBy,
    ) -> impl Future<Output = ()> + use<> {
        let recorded = self.sessions.core().map(|core| {
            crate::audit::tool_decided(&core, &self.project, Some(&self.session), tool, allowed, by)
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
        tool: &ToolRequest,
        answer: PermissionAnswer,
        by: DecidedBy,
    ) -> PermissionAnswer {
        let allowed = answer.decision == Decision::Allow;
        self.audit_tool(tool, allowed, by).await;
        answer
    }

    /// The agent is about to run `tool`; `id` is the agent's id for the
    /// call (Claude: from its `PreToolUse` hook), if it gave one. The
    /// runtime awaits this before the tool runs, so a direct session can
    /// snapshot the tree first (ADR 0005).
    ///
    /// Returns what the project's policy says about the call: the hook also
    /// sees calls the user's own settings allow without asking, which never
    /// reach [`Self::ask`] unless the hook tells the CLI to ask.
    pub async fn before_tool(&self, id: Option<&str>, tool: &ToolRequest) -> BeforeTool {
        let name = tool.name.as_str();
        if let Some(why) = self
            .outside_research(name)
            .or_else(|| self.claimed_by_other(tool))
        {
            self.audit_tool(tool, false, DecidedBy::Role).await;
            return BeforeTool::Deny(why);
        }
        let hold = self.plan_hold(tool);
        if hold == plan::Hold::Refuse {
            self.audit_tool(tool, false, DecidedBy::Role).await;
            return BeforeTool::Deny(Self::plan_refusal());
        }
        // Ovra's own tools follow their role rules (`Self::ask`).
        let mut verdict = if hold == plan::Hold::Ask {
            BeforeTool::Ask(plan::ASKS.to_owned())
        } else {
            BeforeTool::Proceed
        };
        if !name.starts_with(crate::orchestration::IDE_TOOL_PREFIX) {
            let refused = match self.policy(tool) {
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
                self.audit_tool(tool, false, by).await;
                return BeforeTool::Deny(why);
            }
        }
        if let Some(id) = id {
            self.sessions
                .direct
                .tool_started(&self.session, id, tool)
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
        if let Ok(((message, seq), stuck)) = self
            .sessions
            .find(&self.session, |s| (s.push(message), s.stuck_changed()))
        {
            self.sink.emit(AgentsEvent::Message {
                session: self.session.clone(),
                message,
                seq,
            });
            if let Some(stuck) = stuck {
                self.stuck(stuck);
            }
        }
    }

    /// Shows the next piece of the assistant text the agent is writing. Not
    /// kept: the complete message comes through [`Self::message`].
    pub fn text_delta(&self, text: String) {
        let stuck = self.sessions.find(&self.session, |s| {
            s.session.watch.active();
            s.stuck_changed()
        });
        if let Ok(Some(stuck)) = stuck {
            self.stuck(stuck);
        }
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
            self.sessions.record_turns(&self.session);
        }
        let idle = status == AgentStatus::Idle;
        let changed = self.set_status(status);
        if changed && ended {
            let dropped = self.sessions.permissions.drop_session(&self.session);
            self.resolved(dropped, Decision::Deny);
        }
        if changed && idle && self.sessions.direct.turn_ended(&self.session) {
            self.sessions.record_turns(&self.session);
        }
        if changed && ended {
            self.drop_queue();
        }
        if changed && idle {
            self.send_queued();
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
            (changed, s.run(), s.stuck_changed())
        });
        // The run that ended: a resume may have started the next one since.
        let (changed, run, stuck) = changed.unwrap_or((false, 0, None));
        if changed {
            self.sink.emit(AgentsEvent::Status {
                session: self.session.clone(),
                project: self.project.clone(),
                status: status.clone(),
            });
            if let Some(stuck) = stuck {
                self.stuck(stuck);
            }
            if let Some(core) = self.sessions.core() {
                if status == AgentStatus::Working {
                    stuck::wake(&core);
                }
                crate::resources::wake(&core);
                crate::orchestration::on_status(&core, &self.session, &status);
                crate::power::sessions_changed(&core);
                if status.is_ended() {
                    crate::servers::session_ended(&core, &self.session, run);
                    crate::browser::session_ended(&core, &self.session);
                }
            }
        }
        changed
    }

    fn stuck(&self, stuck: Option<crate::protocol::agents::Stuck>) {
        self.sink.emit(AgentsEvent::Stuck {
            session: self.session.clone(),
            stuck,
        });
    }

    pub(crate) fn resolved(&self, ids: Vec<PermissionId>, decision: Decision) {
        for id in ids {
            self.sink.emit(PermissionsEvent::Resolved { id, decision });
        }
    }

    /// Asks a person whether the agent may make the call `tool`, and
    /// waits for the answer. The session shows as waiting meanwhile. A
    /// stopping or ended session is denied without asking. Dropping the
    /// future withdraws the request.
    pub async fn ask(
        &self,
        tool: ToolRequest,
        title: Option<String>,
        reason: Option<PermissionReason>,
    ) -> PermissionAnswer {
        if let Some(why) = self
            .outside_research(&tool.name)
            .or_else(|| self.claimed_by_other(&tool))
        {
            let answer = PermissionAnswer::deny(why);
            return self.decided(&tool, answer, DecidedBy::Role).await;
        }
        let hold = self.plan_hold(&tool);
        if hold == plan::Hold::Refuse {
            let answer = PermissionAnswer::deny(Self::plan_refusal());
            return self.decided(&tool, answer, DecidedBy::Role).await;
        }
        if let Some(name) = tool
            .name
            .strip_prefix(crate::orchestration::IDE_TOOL_PREFIX)
        {
            // Ovra's own tools are decided by role and limits, never asked.
            return match self.sessions.core() {
                Some(core) => {
                    let answer =
                        crate::orchestration::decide(&core, &self.session, name, &tool.input);
                    self.decided(&tool, answer, DecidedBy::Role).await
                }
                None => PermissionAnswer::deny("Ovra is shutting down"),
            };
        }
        let decided = self.policy(&tool);
        // A call some rule asks about, or under an unreadable policy, gets
        // no "Always allow"; nor does one plan first holds.
        let open = matches!(decided, Ok(None)) && hold == plan::Hold::None;
        match decided {
            Ok(Some((PolicyAction::Deny, rule))) => {
                // The hook may have seen the call first (a policy changed
                // between the two).
                self.sessions
                    .direct
                    .tool_answered(&self.session, &tool, false)
                    .await;
                let answer = PermissionAnswer::deny(format!(
                    "The project's policy refuses this ({rule}). Ask the person if it is needed."
                ));
                return self
                    .decided(&tool, answer, DecidedBy::Policy { rule })
                    .await;
            }
            // Plan first puts commands to the person, whatever a rule allows.
            Ok(Some((PolicyAction::Auto, rule))) if hold == plan::Hold::None => {
                let by = DecidedBy::Policy { rule };
                return self.decided(&tool, PermissionAnswer::allow(), by).await;
            }
            // A research session's web search runs on Anthropic's side and
            // touches nothing here (ADR 0013).
            Ok(None) if tool.name == WEB_SEARCH && self.is_research() => {
                return self
                    .decided(&tool, PermissionAnswer::allow(), DecidedBy::Role)
                    .await;
            }
            // Unreadable policy: ask, never decide on its own.
            _ => {}
        }
        let always = |cwd: Option<&Path>, sandbox: Option<&SandboxReport>| {
            cwd.filter(|_| open)
                .and_then(|cwd| crate::policy::always_rule(&tool.name, &tool.call, cwd, sandbox))
        };
        let reason = match hold {
            plan::Hold::Ask => Some(PermissionReason {
                text: plan::ASKS.to_owned(),
                kind: None,
            }),
            _ => reason,
        };
        self.ask_person(&tool, title, reason, always).await
    }

    /// Puts `tool` to the person and waits for the answer, as [`Self::ask`]
    /// does once no rule decided it; `always` gives the rule "Always
    /// allow" offers, from the session's folder and sandbox.
    pub(crate) async fn ask_person(
        &self,
        tool: &ToolRequest,
        title: Option<String>,
        reason: Option<PermissionReason>,
        always: impl FnOnce(Option<&Path>, Option<&SandboxReport>) -> Option<PolicyRule>,
    ) -> PermissionAnswer {
        self.ask_person_by(tool, title, reason, always, false)
            .await
            .0
    }

    /// [`Self::ask_person`], with who answered. With `keep_idle`, an idle
    /// session (a key a background process wants) stays idle while it waits.
    pub(crate) async fn ask_person_by(
        &self,
        tool: &ToolRequest,
        title: Option<String>,
        reason: Option<PermissionReason>,
        always: impl FnOnce(Option<&Path>, Option<&SandboxReport>) -> Option<PolicyRule>,
        keep_idle: bool,
    ) -> (PermissionAnswer, DecidedBy) {
        let tool = tool.clone();
        let mut idle = false;
        let registered = self.sessions.find(&self.session, |s| {
            if s.stopping() {
                return Err(PermissionAnswer::deny("the session was stopped"));
            }
            if s.status().is_ended() {
                return Err(PermissionAnswer::deny("the session ended"));
            }
            idle = keep_idle && *s.status() == AgentStatus::Idle;
            let cwd = s.launch().map(|l| l.cwd);
            let sandbox = s.info().sandbox;
            let always = always(cwd.as_deref(), sandbox.as_ref());
            Ok(s.permissions.register(
                PermissionRequest {
                    id: PermissionId(String::new()),
                    session: self.session.clone(),
                    project: s.info().project,
                    cwd: cwd.map(|c| c.to_string_lossy().into_owned()),
                    tool: tool.name.clone(),
                    input: tool.input.clone(),
                    call: tool.call.clone(),
                    title,
                    reason,
                    always,
                },
                !idle,
            ))
        });
        let (request, answer) = match registered {
            Ok(Ok(registered)) => registered,
            Ok(Err(answer)) => {
                let by = DecidedBy::SessionEnded;
                return (self.decided(&tool, answer, by.clone()).await, by);
            }
            Err(_) => {
                let answer = PermissionAnswer::deny("the session ended");
                let by = DecidedBy::SessionEnded;
                return (self.decided(&tool, answer, by.clone()).await, by);
            }
        };
        let mut waiting = Waiting {
            host: self,
            id: Some(request.id.clone()),
            settled: false,
            idle,
        };
        let direct = &self.sessions.direct;
        direct.tool_waiting(&self.session, &tool).await;
        self.sink
            .emit(PermissionsEvent::Requested(Box::new(request)));
        if !idle {
            self.set_status(AgentStatus::WaitingForApproval);
        }
        let (answer, by) = answer.await.unwrap_or_else(|_| {
            (
                PermissionAnswer::deny("the session ended"),
                DecidedBy::SessionEnded,
            )
        });
        waiting.id = None;
        // Queued at once, so the entry survives the future being dropped.
        let allowed = answer.decision == Decision::Allow;
        let recorded = self.audit_tool(&tool, allowed, by.clone());
        direct.tool_answered(&self.session, &tool, allowed).await;
        // Ignored while other requests of the session still wait.
        if !idle {
            self.set_status(AgentStatus::Working);
        }
        waiting.settled = true;
        recorded.await;
        (answer, by)
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

    /// Why a worker's file change is refused because another worker of
    /// its task holds the file on the board (`docs/specs/task-board.md`).
    fn claimed_by_other(&self, tool: &ToolRequest) -> Option<String> {
        if !matches!(
            tool.call.kind,
            ToolKind::Edit | ToolKind::Delete | ToolKind::Move
        ) {
            return None;
        }
        let core = self.sessions.core()?;
        let cwd = self
            .sessions
            .find(&self.session, |s| s.launch().map(|l| l.cwd))
            .ok()
            .flatten()?;
        crate::orchestration::claimed_write(&core, &self.session, &tool.call, &cwd)
    }

    /// Why `tool` is refused, if this is a research session and the tool is
    /// not one of [`RESEARCH_TOOLS`].
    fn outside_research(&self, tool: &str) -> Option<String> {
        let Ok((research, no_web)) = self.sessions.find(&self.session, |s| {
            let info = s.info();
            (info.mode == SessionMode::Research, info.no_web)
        }) else {
            // Fails closed: a session that is gone runs nothing.
            return Some("The session has ended.".to_owned());
        };
        let allowed = RESEARCH_TOOLS.contains(&tool) && !(no_web && tool == WEB_SEARCH);
        (research && !allowed).then(|| {
            let can = if no_web {
                "only reads the project"
            } else {
                "only reads the project and searches the web"
            };
            format!(
                "A research session {can}; {tool} is not available. Put what should change \
                 in your report instead."
            )
        })
    }
}

/// A tool call as a runtime passes it to the core ([`AgentHost::ask`],
/// [`AgentHost::before_tool`]): the agent's own name and input, and what
/// they mean in shared terms. The runtime maps its tools; the core decides
/// on `call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRequest {
    pub name: String,
    pub input: serde_json::Value,
    pub call: ToolCall,
    /// The text an `edit` call writes (a file's new content, or what
    /// replaces a part of it), whose hash the audit log keeps. Never sent
    /// to clients or kept in transcripts.
    pub written: Option<String>,
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
    /// The session was idle when asked: it stays so.
    idle: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        match self.id.take() {
            Some(id) => {
                if self.host.sessions.permissions.withdraw(&id) {
                    self.host.resolved(vec![id], Decision::Deny);
                    if !self.idle {
                        self.host.set_status(AgentStatus::Working);
                    }
                }
            }
            None if !self.settled && !self.idle => {
                self.host.set_status(AgentStatus::Working);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
