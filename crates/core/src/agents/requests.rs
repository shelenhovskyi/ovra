//! Handling of `agents.*` and `permissions.*` requests.

use std::path::PathBuf;
use std::sync::Arc;

use super::{AgentHost, Sessions};
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, AgentsRequest, AgentsResponse, SessionInfo,
    SessionMode, TurnUsage,
};
use crate::protocol::permissions::{PermissionsEvent, PermissionsRequest, PermissionsResponse};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{AgentRuntime, AgentSpec, EventSink, ProjectStore};

type Result<T> = std::result::Result<T, ProtocolError>;

/// What the agent handlers need from the core.
pub(crate) struct Ctx<'a> {
    pub(crate) sessions: &'a Arc<Sessions>,
    pub(crate) runtime: &'a dyn AgentRuntime,
    pub(crate) projects: &'a dyn ProjectStore,
    pub(crate) sink: EventSink,
}

const OK: Response = Response::Ok(OkResponse::Ok);

pub(crate) fn agents(ctx: &Ctx<'_>, request: AgentsRequest) -> Result<Response> {
    let sessions = ctx.sessions;
    match request {
        AgentsRequest::List { project } => {
            ctx.projects.get(&project)?;
            Ok(Response::Agents(AgentsResponse::List(
                sessions.list(&project),
            )))
        }
        AgentsRequest::Start {
            project,
            agent,
            mode,
            prompt,
        } => {
            let root = PathBuf::from(ctx.projects.get(&project)?.root);
            if mode == SessionMode::Isolated {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "isolated sessions need worktrees, which arrive in phase 3",
                ));
            }
            if sessions.any_running(|s| s.project == project && s.mode == SessionMode::Direct) {
                return Err(ProtocolError::new(
                    ErrorCode::Conflict,
                    "another agent already works in this project's main checkout",
                ));
            }
            let id = sessions.next_id();
            let info = SessionInfo {
                id: id.clone(),
                project: project.clone(),
                agent,
                mode,
                status: AgentStatus::Starting,
                title: title(&prompt),
                usage: TurnUsage::default(),
            };
            sessions.insert(info.clone());
            ctx.sink.emit(AgentsEvent::Status {
                session: id.clone(),
                project: project.clone(),
                status: AgentStatus::Starting,
            });
            let host = AgentHost::new(id.clone(), project, sessions.clone(), ctx.sink.clone());
            host.message(AgentMessage::UserText {
                text: prompt.clone(),
            });
            let spec = AgentSpec {
                session: id.clone(),
                agent,
                mode,
                cwd: root,
                prompt,
            };
            match ctx.runtime.start(spec, host.clone()) {
                Ok(process) => sessions.find(&id, |s| s.set_process(Arc::from(process)))?,
                Err(err) => {
                    host.status(AgentStatus::Failed {
                        message: err.message.clone(),
                    });
                    return Err(err);
                }
            }
            let info = sessions.find(&id, |s| s.info().clone())?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Send { session, text } => {
            let (process, project) =
                sessions.find(&session, |s| (s.process(), s.info().project.clone()))?;
            let process = process
                .ok_or_else(|| ProtocolError::new(ErrorCode::Conflict, "the session has ended"))?;
            let host = AgentHost::new(session, project, sessions.clone(), ctx.sink.clone());
            host.message(AgentMessage::UserText { text: text.clone() });
            host.set_status(AgentStatus::Working);
            process.send(text)?;
            Ok(OK)
        }
        AgentsRequest::Stop { session } => {
            let (process, project) =
                sessions.find(&session, |s| (s.process(), s.info().project.clone()))?;
            match process {
                Some(process) => process.stop(),
                None => AgentHost::new(session, project, sessions.clone(), ctx.sink.clone())
                    .status(AgentStatus::Stopped),
            }
            Ok(OK)
        }
        AgentsRequest::Transcript { session } => {
            let transcript = sessions.find(&session, |s| s.transcript().to_vec())?;
            Ok(Response::Agents(AgentsResponse::Transcript(transcript)))
        }
    }
}

pub(crate) fn permissions(ctx: &Ctx<'_>, request: PermissionsRequest) -> Result<Response> {
    let permissions = &ctx.sessions.permissions;
    match request {
        PermissionsRequest::Pending => Ok(Response::Permissions(PermissionsResponse::Pending(
            permissions.pending(),
        ))),
        PermissionsRequest::Answer { id, answer } => {
            let decision = answer.decision;
            if !permissions.answer(&id, answer) {
                return Err(ProtocolError::new(
                    ErrorCode::NotFound,
                    format!("permission request {} is not pending", id.0),
                ));
            }
            ctx.sink.emit(PermissionsEvent::Resolved { id, decision });
            Ok(OK)
        }
    }
}

/// The first line of the prompt, at most 80 characters.
fn title(prompt: &str) -> String {
    let line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    match line.char_indices().nth(80) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}
