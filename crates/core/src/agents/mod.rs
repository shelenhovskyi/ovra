//! Agent sessions: the registry, their transcripts and the [`AgentHost`]
//! runtimes report through. Runtimes themselves live behind
//! [`AgentRuntime`](crate::services::AgentRuntime).

mod permissions;
mod requests;
mod state;
mod transcript;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, SessionId, SessionInfo};
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionId, PermissionReason, PermissionsEvent,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{
    EventSink, IdeTools, RunningAgent, SessionStore, StoredSession, ToolCaller, ToolRole,
};

pub(crate) use permissions::Permissions;
pub(crate) use requests::{
    Ctx, agents as handle_agents, permissions as handle_permissions, stop_project, trust,
};
use transcript::Transcript;

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
    pub(crate) fn new(store: Arc<dyn SessionStore>) -> Self {
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
        }
    }

    pub(crate) fn link_ide(&self, tools: Arc<dyn IdeTools>, core: crate::core::WeakCore) {
        *lock(&self.ide) = Some(IdeLink { tools, core });
    }

    pub(crate) fn next_id(&self) -> SessionId {
        let mut started = lock(&self.started);
        *started += 1;
        SessionId(format!("s{started}"))
    }

    pub(crate) fn insert(&self, info: SessionInfo) {
        let mut list = lock(&self.list);
        let project = info.project.clone();
        let session = Session {
            info,
            transcript: Transcript::default(),
            process: None,
            stopping: false,
            agent_session: None,
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

    /// Whether any session matching `f` has not ended.
    pub(crate) fn any_running(&self, f: impl Fn(&SessionInfo) -> bool) -> bool {
        lock(&self.list)
            .iter()
            .any(|s| !s.info.status.is_ended() && f(&s.info))
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
    fn set_agent_session(&mut self, id: String) {
        if self.session.agent_session.as_ref() != Some(&id) {
            self.session.agent_session = Some(id);
            self.save();
        }
    }
    /// Starts an ended session over for `agents.resume`.
    pub(crate) fn restart(&mut self) {
        self.session.info.status = AgentStatus::Starting;
        self.session.stopping = false;
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

    /// Adds a message to the transcript and shows it.
    pub fn message(&self, message: AgentMessage) {
        if let Ok(message) = self.sessions.find(&self.session, |s| s.push(message)) {
            self.sink.emit(AgentsEvent::Message {
                session: self.session.clone(),
                message,
            });
        }
    }

    /// Records a new state, if the state machine allows it. An ended session
    /// drops its pending permission requests, which the agent sees as denied.
    pub fn status(&self, status: AgentStatus) {
        let ended = status.is_ended();
        if self.set_status(status) && ended {
            let dropped = self.sessions.permissions.drop_session(&self.session);
            self.resolved(dropped, Decision::Deny);
        }
    }

    pub(crate) fn set_status(&self, status: AgentStatus) -> bool {
        let changed = self
            .sessions
            .find(&self.session, |s| s.set_status(status.clone()));
        if changed == Ok(true) {
            self.sink.emit(AgentsEvent::Status {
                session: self.session.clone(),
                project: self.project.clone(),
                status,
            });
        }
        changed == Ok(true)
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
        let registered = self.sessions.find(&self.session, |s| {
            if s.stopping() {
                return Err(PermissionAnswer::deny("the session was stopped"));
            }
            if s.status().is_ended() {
                return Err(PermissionAnswer::deny("the session ended"));
            }
            Ok(s.permissions
                .register(self.session.clone(), tool, input, title, reason))
        });
        let (request, answer) = match registered {
            Ok(Ok(registered)) => registered,
            Ok(Err(answer)) => return answer,
            Err(_) => return PermissionAnswer::deny("the session ended"),
        };
        let mut waiting = Waiting {
            host: self,
            id: Some(request.id.clone()),
        };
        self.sink.emit(PermissionsEvent::Requested(request));
        self.set_status(AgentStatus::WaitingForApproval);
        let answer = answer
            .await
            .unwrap_or_else(|_| PermissionAnswer::deny("the session ended"));
        waiting.id = None;
        // Ignored while other requests of the session still wait.
        self.set_status(AgentStatus::Working);
        answer
    }
}

/// Withdraws the request when the agent stops waiting before it is answered
/// (the [`AgentHost::ask`] future is dropped, e.g. the agent cancelled it).
struct Waiting<'a> {
    host: &'a AgentHost,
    id: Option<PermissionId>,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else { return };
        if self.host.sessions.permissions.withdraw(&id) {
            self.host.resolved(vec![id], Decision::Deny);
            self.host.set_status(AgentStatus::Working);
        }
    }
}

#[cfg(test)]
mod tests;
