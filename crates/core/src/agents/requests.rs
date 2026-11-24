//! Handling of `agents.*` and `permissions.*` requests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::transcript::MAX_TEXT_BYTES;
use super::{AgentHost, Launch, Sessions};
use crate::protocol::agents::{
    AgentKind, AgentMessage, AgentStatus, AgentsEvent, AgentsRequest, AgentsResponse, MessageFrom,
    ProjectConfigReview, SessionId, SessionInfo, SessionMode, TurnUsage,
};
use crate::protocol::audit::DecidedBy;
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionsEvent, PermissionsRequest, PermissionsResponse,
};
use crate::protocol::projects::{Project, ProjectId};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{
    AgentRuntime, AgentSpec, EventSink, ProfileNetwork, ProjectStore, SandboxProfile, ToolRole,
};

type Result<T> = std::result::Result<T, ProtocolError>;

/// What the agent handlers need from the core.
pub(crate) struct Ctx<'a> {
    pub(crate) sessions: &'a Arc<Sessions>,
    pub(crate) runtime: &'a dyn AgentRuntime,
    pub(crate) projects: &'a dyn ProjectStore,
    pub(crate) sink: EventSink,
}

const OK: Response = Response::Ok(OkResponse::Ok);

pub(crate) async fn agents(ctx: &Ctx<'_>, request: AgentsRequest) -> Result<Response> {
    let sessions = ctx.sessions;
    match request {
        AgentsRequest::Setup { agent: _ } => {
            let path = ctx.projects.agent_settings()?.claude_path;
            let setup = ctx.runtime.setup(path.map(PathBuf::from)).await;
            Ok(Response::Agents(AgentsResponse::Setup(setup)))
        }
        AgentsRequest::SetPath { agent: _, path } => {
            let path = path.filter(|p| !p.trim().is_empty());
            if path.as_deref().is_some_and(|p| !Path::new(p).is_absolute()) {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "the path must be absolute",
                ));
            }
            let mut settings = ctx.projects.agent_settings()?;
            settings.claude_path.clone_from(&path);
            ctx.projects.set_agent_settings(settings)?;
            let setup = ctx.runtime.setup(path.map(PathBuf::from)).await;
            Ok(Response::Agents(AgentsResponse::Setup(setup)))
        }
        AgentsRequest::ProjectConfig { project } => {
            let review = review(ctx, &project).await?;
            Ok(Response::Agents(AgentsResponse::ProjectConfig(review)))
        }
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
            if mode == SessionMode::Isolated {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "isolated sessions are started by a coordinator (orchestration.start)",
                ));
            }
            let new = NewSession {
                project,
                agent,
                mode,
                prompt,
                cwd: None,
                tools: None,
                from: MessageFrom::Person,
            };
            let info = start(ctx, new).await?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Resume { session, prompt } => {
            let info = resume(ctx, session, vec![(prompt, MessageFrom::Person)]).await?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Send { session, text } => {
            send(ctx, session, vec![(text, MessageFrom::Person)])?;
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
            // The person interrupted the turn, which refuses what it asked.
            let denied = sessions.permissions.deny_session(
                &session,
                &PermissionAnswer::deny("the turn was interrupted"),
                &DecidedBy::Person,
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

/// A session to start.
pub(crate) struct NewSession {
    pub(crate) project: ProjectId,
    pub(crate) agent: AgentKind,
    pub(crate) mode: SessionMode,
    pub(crate) prompt: String,
    /// Where it works; `None` for the project root.
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) tools: Option<ToolRole>,
    /// Who wrote `prompt`.
    pub(crate) from: MessageFrom,
}

/// Starts a session and returns it as it is now.
pub(crate) async fn start(ctx: &Ctx<'_>, new: NewSession) -> Result<SessionInfo> {
    check_text(&new.prompt)?;
    let target = prepare(ctx, &new.project, new.mode, new.tools).await?;
    let launch = Launch {
        cwd: new
            .cwd
            .unwrap_or_else(|| PathBuf::from(&target.project.root)),
        tools: new.tools,
    };
    let id = ctx.sessions.next_id();
    let base = baseline(ctx, &new.project, &id, new.mode, &launch).await?;
    ctx.sessions.insert(SessionInfo {
        id: id.clone(),
        project: new.project,
        agent: new.agent,
        mode: new.mode,
        status: AgentStatus::Starting,
        title: title(&new.prompt),
        usage: TurnUsage::default(),
        resumable: false,
        billing: None,
        sandbox: None,
    });
    review_from(ctx, &id, &launch, base);
    let target = trusted_in(ctx, target, &launch.cwd).await;
    run(ctx, &id, target, launch, vec![(new.prompt, new.from)], None).await
}

/// The project a session may start in, with what the runtime needs to know.
struct Target {
    project: Project,
    executable: Option<String>,
    trusted: bool,
}

async fn prepare(
    ctx: &Ctx<'_>,
    project: &ProjectId,
    mode: SessionMode,
    tools: Option<ToolRole>,
) -> Result<Target> {
    let target = ctx.projects.get(project)?;
    let executable = ctx.projects.agent_settings()?.claude_path;
    // Settings that changed since the user trusted them (or cannot be read)
    // do not load until the user reviews them again.
    let trusted = target.trusted && review(ctx, project).await.is_ok_and(|r| r.applies());
    // A coordinator only reads the main checkout, so it does not take the
    // direct writer's place (docs/specs/orchestration.md).
    let writes_main = mode == SessionMode::Direct && tools != Some(ToolRole::Coordinator);
    if writes_main && ctx.sessions.direct_writer(project) {
        return Err(conflict(
            "another agent already works in this project's main checkout",
        ));
    }
    Ok(Target {
        project: target,
        executable,
        trusted,
    })
}

/// A direct session that writes takes the project's direct-writer lock and
/// records its baseline (ADR 0005), which its review compares against.
/// Without a baseline (not a git repository, worktrees not wired) it still
/// runs and holds the lock, but has nothing to review.
async fn baseline(
    ctx: &Ctx<'_>,
    project: &ProjectId,
    id: &SessionId,
    mode: SessionMode,
    launch: &Launch,
) -> Result<Option<String>> {
    if mode != SessionMode::Direct || launch.tools == Some(ToolRole::Coordinator) {
        return Ok(None);
    }
    let direct = &ctx.sessions.direct;
    match direct.begin(project, id, launch.cwd.clone()).await {
        Ok(base) => Ok(Some(base)),
        Err(refused) if direct.writer(project).as_ref() != Some(id) => Err(refused),
        Err(_) => Ok(None),
    }
}

/// Gives a direct session with a baseline its review item.
fn review_from(ctx: &Ctx<'_>, id: &SessionId, launch: &Launch, base: Option<String>) {
    if let (Some(base), Some(core)) = (base, ctx.sessions.core()) {
        crate::orchestration::direct::review_from(&core, id, launch.cwd.clone(), base);
    }
}

/// A message to an agent in parts, each with who wrote it: the agent gets
/// them as one text, the transcript shows each part with its author.
pub(crate) type Parts = Vec<(String, MessageFrom)>;

fn joined(parts: &Parts) -> Result<String> {
    let text = parts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    check_text(&text)?;
    Ok(text)
}

/// Resumes the ended session `session` with `parts` as its next prompt.
pub(crate) async fn resume(ctx: &Ctx<'_>, session: SessionId, parts: Parts) -> Result<SessionInfo> {
    joined(&parts)?;
    let sessions = ctx.sessions;
    let (info, agent_session, launch) =
        sessions.find(&session, |s| (s.info(), s.agent_session(), s.launch()))?;
    if !info.status.is_ended() {
        return Err(conflict("the session is still running"));
    }
    let agent_session = agent_session
        .ok_or_else(|| conflict("the agent did not report how to resume this session"))?;
    if info.mode == SessionMode::Isolated && launch.is_none() {
        return Err(conflict(
            "this worker's worktree is not known since Ovra restarted",
        ));
    }
    let tools = launch.as_ref().and_then(|l| l.tools);
    let target = prepare(ctx, &info.project, info.mode, tools).await?;
    let launch = launch.unwrap_or(Launch {
        cwd: PathBuf::from(&target.project.root),
        tools: None,
    });
    let base = baseline(ctx, &info.project, &session, info.mode, &launch).await?;
    review_from(ctx, &session, &launch, base);
    let restart = || sessions.find(&session, |s| s.restart());
    match sessions.core() {
        // Checked together with the restart, so a worktree being removed
        // is never resumed into.
        Some(core) => crate::orchestration::unless_cleaned(&core, &session, restart)??,
        None => restart()?,
    }
    let target = trusted_in(ctx, target, &launch.cwd).await;
    run(ctx, &session, target, launch, parts, Some(agent_session)).await
}

/// What a session may touch in a sandbox. A worker reads its main
/// repository's git folder, which its worktree's history lives in.
fn profile(
    project: &Project,
    mode: SessionMode,
    cwd: &Path,
    network: ProfileNetwork,
) -> SandboxProfile {
    SandboxProfile {
        work_dir: cwd.to_path_buf(),
        read: match mode {
            SessionMode::Isolated => vec![PathBuf::from(&project.root).join(".git")],
            SessionMode::Direct => Vec::new(),
        },
        write: Vec::new(),
        network,
    }
}

/// Starts the runtime for session `id`, which is `starting`, with `parts`.
async fn run(
    ctx: &Ctx<'_>,
    id: &SessionId,
    target: Target,
    launch: Launch,
    parts: Parts,
    resume: Option<String>,
) -> Result<SessionInfo> {
    let prompt = joined(&parts)?;
    let sessions = ctx.sessions;
    let project = target.project.id.clone();
    let run = sessions.find(id, |s| {
        s.set_launch(launch.clone());
        s.set_trusted(target.trusted);
        s.run()
    })?;
    ctx.sink.emit(AgentsEvent::Status {
        session: id.clone(),
        project: project.clone(),
        status: AgentStatus::Starting,
    });
    if let Some(core) = sessions.core() {
        crate::orchestration::on_status(&core, id, &AgentStatus::Starting);
    }
    let host = AgentHost::new(
        id.clone(),
        project.clone(),
        sessions.clone(),
        ctx.sink.clone(),
    );
    for (text, from) in parts {
        host.message(AgentMessage::UserText { text, from });
    }
    let (agent, mode) = sessions.find(id, |s| {
        let info = s.info();
        (info.agent, info.mode)
    })?;
    let network = sessions
        .net_start(ctx.projects, &project, id, run, ctx.sink.clone())
        .await;
    let network = match network {
        Ok(network) => network,
        // Another run owns the session now: leave its state alone.
        Err(err) if sessions.find(id, |s| s.run() != run)? => return Err(err),
        Err(err) => {
            host.status(AgentStatus::Failed {
                message: format!("the session's netproxy did not start: {}", err.message),
            });
            return Err(err);
        }
    };
    let sandbox = profile(&target.project, mode, &launch.cwd, network);
    let spec = AgentSpec {
        session: id.clone(),
        agent,
        mode,
        cwd: launch.cwd,
        tools: launch.tools,
        prompt,
        executable: target.executable.map(PathBuf::from),
        trusted: target.trusted,
        resume,
        sandbox,
    };
    match ctx.runtime.start(spec, host.clone()) {
        Ok(process) => sessions.find(id, |s| s.set_process(Arc::from(process)))?,
        Err(err) => {
            host.status(AgentStatus::Failed {
                message: err.message.clone(),
            });
            return Err(err);
        }
    }
    sessions.find(id, |s| s.info())
}

/// A session outside the project root (a worker's worktree) loads the
/// project's settings only if the copy in its own folder is exactly what the
/// user trusted: an agent could have changed it there.
async fn trusted_in(ctx: &Ctx<'_>, mut target: Target, cwd: &Path) -> Target {
    if target.trusted && cwd != Path::new(&target.project.root) {
        let trusted = ctx
            .projects
            .trusted_config(&target.project.id)
            .ok()
            .flatten();
        let here = ctx.runtime.project_config(cwd.to_path_buf()).await.ok();
        target.trusted =
            matches!((trusted, here), (Some(t), Some(h)) if t.fingerprint == h.fingerprint);
    }
    target
}

/// What the project would load now, and what the user trusted.
async fn review(ctx: &Ctx<'_>, project: &ProjectId) -> Result<ProjectConfigReview> {
    let root = PathBuf::from(ctx.projects.get(project)?.root);
    let current = ctx.runtime.project_config(root).await?;
    let trusted = ctx.projects.trusted_config(project)?;
    Ok(ProjectConfigReview { current, trusted })
}

/// `projects.trust`: trusting records exactly the configuration the user
/// reviewed, identified by its fingerprint.
pub(crate) async fn trust(
    ctx: &Ctx<'_>,
    project: &ProjectId,
    trusted: bool,
    fingerprint: Option<String>,
) -> Result<Project> {
    if !trusted {
        return ctx.projects.set_trusted(project, None);
    }
    let Some(fingerprint) = fingerprint else {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "trusting a project needs the fingerprint of what was reviewed",
        ));
    };
    let current = review(ctx, project).await?.current;
    if current.fingerprint != fingerprint {
        return Err(conflict(
            "the project's settings changed while you reviewed them; review them again",
        ));
    }
    ctx.projects.set_trusted(project, Some(current))
}

/// Sends `parts` as the running session's next message.
pub(crate) fn send(ctx: &Ctx<'_>, session: SessionId, parts: Parts) -> Result<()> {
    let text = joined(&parts)?;
    let sessions = ctx.sessions;
    let (process, project, stopping) =
        sessions.find(&session, |s| (s.process(), s.info().project, s.stopping()))?;
    let process = match process {
        Some(process) if !stopping => process,
        Some(_) => return Err(conflict("the session is stopping")),
        None => return Err(conflict("the session has ended")),
    };
    let host = AgentHost::new(session, project, sessions.clone(), ctx.sink.clone());
    for (text, from) in parts {
        host.message(AgentMessage::UserText { text, from });
    }
    host.set_status(AgentStatus::Working);
    process.send(text)
}

/// Denies what the agent waits for and asks it to end; the runtime then
/// reports `stopped`. Stopping an ended session does nothing.
pub(crate) fn stop(ctx: &Ctx<'_>, session: SessionId) -> Result<()> {
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
pub(crate) fn check_text(text: &str) -> Result<()> {
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
