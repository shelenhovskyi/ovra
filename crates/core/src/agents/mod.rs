//! Agent sessions: the registry, their transcripts and the [`AgentHost`]
//! runtimes report through. Runtimes themselves live behind
//! [`AgentRuntime`](crate::services::AgentRuntime).

mod permissions;
mod requests;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, SessionId, SessionInfo};
use crate::protocol::permissions::{PermissionAnswer, PermissionsEvent};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{EventSink, RunningAgent};

pub(crate) use permissions::Permissions;
pub(crate) use requests::{Ctx, agents as handle_agents, permissions as handle_permissions};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
struct Session {
    info: SessionInfo,
    transcript: Vec<AgentMessage>,
    process: Option<Arc<dyn RunningAgent>>,
}

/// Every session of this core run, in start order.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    list: Mutex<Vec<Session>>,
    started: Mutex<u64>,
    pub(crate) permissions: Permissions,
}

impl Sessions {
    pub(crate) fn next_id(&self) -> SessionId {
        let mut started = lock(&self.started);
        *started += 1;
        SessionId(format!("s{started}"))
    }

    pub(crate) fn insert(&self, info: SessionInfo) {
        lock(&self.list).push(Session {
            info,
            transcript: Vec::new(),
            process: None,
        });
    }

    pub(crate) fn list(&self, project: &ProjectId) -> Vec<SessionInfo> {
        lock(&self.list)
            .iter()
            .filter(|s| &s.info.project == project)
            .map(|s| s.info.clone())
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
        Ok(f(&mut SessionView(session)))
    }

    /// Whether any session matching `f` has not ended.
    pub(crate) fn any_running(&self, f: impl Fn(&SessionInfo) -> bool) -> bool {
        lock(&self.list)
            .iter()
            .any(|s| !s.info.status.is_ended() && f(&s.info))
    }
}

/// Access to one session while the registry is locked.
pub(crate) struct SessionView<'a>(&'a mut Session);

impl SessionView<'_> {
    pub(crate) fn info(&self) -> &SessionInfo {
        &self.0.info
    }
    pub(crate) fn transcript(&self) -> &[AgentMessage] {
        &self.0.transcript
    }
    pub(crate) fn push(&mut self, message: AgentMessage) {
        self.0.transcript.push(message);
    }
    pub(crate) fn set_status(&mut self, status: AgentStatus) {
        if status.is_ended() {
            self.0.process = None;
        }
        self.0.info.status = status;
    }
    pub(crate) fn process(&self) -> Option<Arc<dyn RunningAgent>> {
        self.0.process.clone()
    }
    pub(crate) fn set_process(&mut self, process: Arc<dyn RunningAgent>) {
        if !self.0.info.status.is_ended() {
            self.0.process = Some(process);
        }
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

    /// Adds a message to the transcript and shows it.
    pub fn message(&self, message: AgentMessage) {
        let added = self
            .sessions
            .find(&self.session, |s| s.push(message.clone()));
        if added.is_ok() {
            self.sink.emit(AgentsEvent::Message {
                session: self.session.clone(),
                message,
            });
        }
    }

    /// Records a new state. An ended session drops its pending permission
    /// requests, which the agent sees as denied.
    pub fn status(&self, status: AgentStatus) {
        self.set_status(status.clone());
        if status.is_ended() {
            for id in self.sessions.permissions.drop_session(&self.session) {
                self.sink.emit(PermissionsEvent::Resolved {
                    id,
                    decision: crate::protocol::permissions::Decision::Deny,
                });
            }
        }
    }

    pub(crate) fn set_status(&self, status: AgentStatus) {
        let changed = self.sessions.find(&self.session, |s| {
            if s.info().status.is_ended() || s.info().status == status {
                return false;
            }
            s.set_status(status.clone());
            true
        });
        if changed == Ok(true) {
            self.sink.emit(AgentsEvent::Status {
                session: self.session.clone(),
                project: self.project.clone(),
                status,
            });
        }
    }

    /// Asks a person whether the agent may use `tool` with `input`, and
    /// waits for the answer. The session shows as waiting meanwhile.
    pub async fn ask(
        &self,
        tool: String,
        input: serde_json::Value,
        title: Option<String>,
    ) -> PermissionAnswer {
        let (request, answer) =
            self.sessions
                .permissions
                .register(self.session.clone(), tool, input, title);
        self.sink.emit(PermissionsEvent::Requested(request));
        self.set_status(AgentStatus::WaitingForApproval);
        let answer = answer
            .await
            .unwrap_or_else(|_| PermissionAnswer::deny("the session ended"));
        if !self.sessions.permissions.has_pending(&self.session) {
            self.set_status(AgentStatus::Working);
        }
        answer
    }
}

#[cfg(test)]
mod tests;
