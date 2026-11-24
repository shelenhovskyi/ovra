//! Handling of `agents.*` and `permissions.*` requests.

use std::path::PathBuf;
use std::sync::Arc;

use super::transcript::MAX_TEXT_BYTES;
use super::{AgentHost, Sessions};
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, AgentsRequest, AgentsResponse, SessionId, SessionInfo,
    SessionMode, TurnUsage,
};
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionsEvent, PermissionsRequest, PermissionsResponse,
};
use crate::protocol::projects::ProjectId;
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
            check_text(&prompt)?;
            if mode == SessionMode::Isolated {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "isolated sessions need worktrees, which arrive in phase 3",
                ));
            }
            if sessions.any_running(|s| s.project == project && s.mode == SessionMode::Direct) {
                return Err(conflict(
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
            let info = sessions.find(&id, |s| s.info())?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Send { session, text } => {
            check_text(&text)?;
            let (process, project, stopping) =
                sessions.find(&session, |s| (s.process(), s.info().project, s.stopping()))?;
            let process = match process {
                Some(process) if !stopping => process,
                Some(_) => return Err(conflict("the session is stopping")),
                None => return Err(conflict("the session has ended")),
            };
            let host = AgentHost::new(session, project, sessions.clone(), ctx.sink.clone());
            host.message(AgentMessage::UserText { text: text.clone() });
            host.set_status(AgentStatus::Working);
            process.send(text)?;
            Ok(OK)
        }
        AgentsRequest::Interrupt { session } => {
            let (process, project, stopping) =
                sessions.find(&session, |s| (s.process(), s.info().project, s.stopping()))?;
            let process = match process {
                Some(process) if !stopping => process,
                Some(_) => return Err(conflict("the session is stopping")),
                None => return Err(conflict("the session has ended")),
            };
            process.interrupt()?;
            let denied = sessions.permissions.deny_session(
                &session,
                &PermissionAnswer::deny("the turn was interrupted"),
            );
            AgentHost::new(session, project, sessions.clone(), ctx.sink.clone())
                .resolved(denied, Decision::Deny);
            Ok(OK)
        }
        AgentsRequest::Stop { session } => {
            stop(ctx, session)?;
            Ok(OK)
        }
        AgentsRequest::Transcript { session } => {
            let transcript = sessions.find(&session, |s| s.transcript())?;
            Ok(Response::Agents(AgentsResponse::Transcript(transcript)))
        }
    }
}

/// Denies what the agent waits for and asks it to end; the runtime then
/// reports `stopped`. Stopping an ended session does nothing.
fn stop(ctx: &Ctx<'_>, session: SessionId) -> Result<()> {
    let sessions = ctx.sessions;
    let (process, project, denied) = sessions.find(&session, |s| {
        let denied = if s.status().is_ended() {
            Vec::new()
        } else {
            s.begin_stop()
        };
        (s.process(), s.info().project, denied)
    })?;
    let host = AgentHost::new(session, project, sessions.clone(), ctx.sink.clone());
    host.resolved(denied, Decision::Deny);
    match process {
        Some(process) => process.stop(),
        None => host.status(AgentStatus::Stopped),
    }
    Ok(())
}

/// Stops every session of a project that is being removed.
pub(crate) fn stop_project(ctx: &Ctx<'_>, project: &ProjectId) {
    for session in ctx.sessions.running(project) {
        // A session that ended meanwhile is fine to skip.
        let _ = stop(ctx, session);
    }
}

/// Prompts and messages must say something and fit in the transcript.
fn check_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "the message is empty",
        ));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("the message is longer than {} KiB", MAX_TEXT_BYTES / 1024),
        ));
    }
    Ok(())
}

fn conflict(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
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
