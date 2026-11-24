//! What Ovra's IDE tools do, for `ovra-mcp-server` (task 3.C). Every call
//! names its caller; a coordinator reaches only its own task's workers, and
//! a worker only itself.

use super::{Result, forbidden, not_found, review, tasks};
use crate::Core;
use crate::agents;
use crate::path::RelPath;
use crate::protocol::agents::{AgentMessage, AgentStatus, SessionId};
use crate::protocol::orchestration::{OrchestrationEvent, ShowTarget, Worker};
use crate::protocol::review::{ReviewId, ReviewItem, ReviewState};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{ToolCaller, ToolRole};

/// Most messages one `read_output` call returns.
const MAX_OUTPUT_MESSAGES: usize = 200;

/// A worker as `list_agents` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerState {
    pub worker: Worker,
    pub status: AgentStatus,
    /// Its review item, once it has one.
    pub review: Option<(ReviewId, ReviewState)>,
}

/// A part of a worker's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub messages: Vec<AgentMessage>,
    /// Where the next call continues.
    pub next: usize,
}

impl Core {
    /// `spawn_agent`: starts a worker of the caller's task.
    pub async fn spawn_agent(&self, caller: &ToolCaller, name: &str, task: &str) -> Result<Worker> {
        coordinator(caller)?;
        tasks::spawn(self, &caller.session, name.to_owned(), task.to_owned()).await
    }

    /// `list_agents`: the caller's workers, oldest first.
    pub fn list_agents(&self, caller: &ToolCaller) -> Result<Vec<WorkerState>> {
        coordinator(caller)?;
        let task = tasks::of_coordinator(self, &caller.session)?;
        task.workers
            .into_iter()
            .map(|worker| {
                let status = self
                    .sessions()
                    .find(&worker.session, |s| s.status().clone())?;
                let review = review::of_session(self, &worker.session).map(|r| (r.id, r.state));
                Ok(WorkerState {
                    worker,
                    status,
                    review,
                })
            })
            .collect()
    }

    /// `read_output`: up to `max` messages of a worker's transcript from
    /// index `since`.
    pub fn read_output(
        &self,
        caller: &ToolCaller,
        agent: &SessionId,
        since: usize,
        max: usize,
    ) -> Result<Output> {
        let worker = self.worker_of(caller, agent)?;
        let transcript = self.sessions().find(&worker.session, |s| s.transcript())?;
        let messages: Vec<AgentMessage> = transcript
            .into_iter()
            .skip(since)
            .take(max.clamp(1, MAX_OUTPUT_MESSAGES))
            .collect();
        Ok(Output {
            next: since + messages.len(),
            messages,
        })
    }

    /// `send_message`: the worker's next message; an ended worker resumes.
    pub async fn send_message(
        &self,
        caller: &ToolCaller,
        agent: &SessionId,
        text: &str,
    ) -> Result<()> {
        let worker = self.worker_of(caller, agent)?;
        review::continue_session(self, &worker.session, text.to_owned()).await
    }

    /// `stop_agent`: stops a worker; its worktree and review stay.
    pub fn stop_agent(&self, caller: &ToolCaller, agent: &SessionId) -> Result<()> {
        let worker = self.worker_of(caller, agent)?;
        agents::stop(&self.agents_ctx(), worker.session)
    }

    /// `request_review`: puts a worker's work in the inbox with `summary`.
    /// A worker names no agent; a coordinator names one of its workers.
    pub async fn request_review(
        &self,
        caller: &ToolCaller,
        agent: Option<&SessionId>,
        summary: &str,
    ) -> Result<ReviewItem> {
        let session = match (caller.role, agent) {
            (ToolRole::Worker, None) => caller.session.clone(),
            (ToolRole::Worker, Some(_)) => {
                return Err(forbidden(
                    "a worker may ask for a review of its own work only",
                ));
            }
            (ToolRole::Coordinator, Some(agent)) => self.worker_of(caller, agent)?.session,
            (ToolRole::Coordinator, None) => {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "name the worker whose work to review (agent_id)",
                ));
            }
        };
        if summary.trim().is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                "the summary is empty",
            ));
        }
        let entry = review::request(self, &session, summary.to_owned())?;
        review::item(self, &entry).await
    }

    /// `open_in_ui`: asks the UI to show something of the caller's project.
    pub fn open_in_ui(&self, caller: &ToolCaller, target: ShowTarget) -> Result<()> {
        let target = match target {
            ShowTarget::File { path, line } => ShowTarget::File {
                path: shown_file(&path)?,
                line,
            },
            ShowTarget::Agent { session } => {
                let project = self.sessions().find(&session, |s| s.info().project)?;
                if project != caller.project {
                    return Err(not_found(format!("session {}", session.0)));
                }
                ShowTarget::Agent { session }
            }
            ShowTarget::Review { id } => {
                let found = self
                    .orchestration_state()
                    .lock()
                    .reviews
                    .iter()
                    .any(|r| r.id == id && r.project == caller.project);
                if !found {
                    return Err(not_found(format!("review {}", id.0)));
                }
                ShowTarget::Review { id }
            }
        };
        self.emit(OrchestrationEvent::Show {
            project: caller.project.clone(),
            target,
        });
        Ok(())
    }

    /// A worker of the caller's task.
    fn worker_of(&self, caller: &ToolCaller, agent: &SessionId) -> Result<Worker> {
        coordinator(caller)?;
        tasks::of_coordinator(self, &caller.session)?
            .workers
            .into_iter()
            .find(|w| &w.session == agent)
            .ok_or_else(|| not_found(format!("worker {} in this task", agent.0)))
    }
}

fn coordinator(caller: &ToolCaller) -> Result<()> {
    match caller.role {
        ToolRole::Coordinator => Ok(()),
        ToolRole::Worker => Err(forbidden("only the task's coordinator may manage workers")),
    }
}

/// The project-relative file `raw` names for `open_in_ui`. Beyond what
/// [`RelPath::parse`] refuses (absolute, UNC, rooted or drive paths, `..`),
/// a segment starting with `~`, `$` or `%` is refused too: a shell or a
/// person reads it as a home folder or a variable, so it cannot be told to
/// stay inside the project. The approval cards treat these the same way.
pub(super) fn shown_file(raw: &str) -> Result<String> {
    let path = RelPath::parse(raw)?;
    if path
        .as_str()
        .split('/')
        .any(|s| s.starts_with(['~', '$', '%']))
    {
        return Err(forbidden(format!(
            "path {raw:?} may name a home folder or a variable"
        )));
    }
    Ok(path.to_string())
}
