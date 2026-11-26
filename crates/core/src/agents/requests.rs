//! Handling of `agents.*` and `permissions.*` requests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::accounts;
use super::transcript::MAX_TEXT_BYTES;
use super::{AgentHost, Launch, Sessions};
use crate::protocol::agents::{
    AgentKind, AgentMessage, AgentStatus, AgentsEvent, AgentsRequest, AgentsResponse,
    DEFAULT_ACCOUNT, MessageFrom, ProjectConfigReview, SessionId, SessionInfo, SessionMode,
    SessionTranscript, SetupState, TurnUsage,
};
use crate::protocol::audit::DecidedBy;
use crate::protocol::permissions::{
    Decision, PermissionAnswer, PermissionsEvent, PermissionsRequest, PermissionsResponse,
};
use crate::protocol::projects::{Project, ProjectId};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{
    AgentRuntime, AgentSpec, EventSink, ProfileNetwork, ProjectStore, SandboxProfile, Secrets,
    ToolRole,
};

type Result<T> = std::result::Result<T, ProtocolError>;

/// What the agent handlers need from the core.
pub(crate) struct Ctx<'a> {
    pub(crate) sessions: &'a Arc<Sessions>,
    pub(crate) runtime: &'a dyn AgentRuntime,
    pub(crate) projects: &'a dyn ProjectStore,
    pub(crate) secrets: &'a dyn Secrets,
    pub(crate) sink: EventSink,
}

const OK: Response = Response::Ok(OkResponse::Ok);

/// `by`: who sends it, for the audit log of what an interrupt refuses.
pub(crate) async fn agents(
    ctx: &Ctx<'_>,
    request: AgentsRequest,
    by: DecidedBy,
) -> Result<Response> {
    let sessions = ctx.sessions;
    match request {
        AgentsRequest::Providers => Ok(Response::Agents(AgentsResponse::Providers(
            ctx.runtime.providers(),
        ))),
        AgentsRequest::Setup { agent } => {
            let settings = ctx.projects.agent_settings()?;
            let path = settings.path(agent).map(PathBuf::from);
            let setup = ctx.runtime.setup(agent, path, None).await;
            Ok(Response::Agents(AgentsResponse::Setup(setup)))
        }
        AgentsRequest::SetPath { agent, path } => {
            let path = path.filter(|p| !p.trim().is_empty());
            if path.as_deref().is_some_and(|p| !Path::new(p).is_absolute()) {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "the path must be absolute",
                ));
            }
            let setup = ctx
                .runtime
                .setup(agent, path.clone().map(PathBuf::from), None)
                .await;
            // A path that leads nowhere is not saved: it would only make every
            // later session fail (a typo, or the name of a shell alias).
            if let (Some(path), SetupState::NotFound) = (&path, &setup.state) {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    format!(
                        "There is no file at {path}. Choose the {program} program itself; \
                         a shell alias or function cannot be used. \
                         `which {program}` in a terminal prints its path.",
                        program = program(agent),
                    ),
                ));
            }
            let mut settings = ctx.projects.agent_settings()?;
            settings.set_path(agent, path);
            ctx.projects.set_agent_settings(settings)?;
            Ok(Response::Agents(AgentsResponse::Setup(setup)))
        }
        AgentsRequest::Accounts { project } => {
            let overview = accounts::overview(ctx, project.as_ref()).await?;
            Ok(Response::Agents(AgentsResponse::Accounts(overview)))
        }
        AgentsRequest::SaveAccount { account } => {
            let mut settings = ctx.projects.agent_settings()?;
            accounts::save(&mut settings, account)?;
            ctx.projects.set_agent_settings(settings)?;
            let overview = accounts::overview(ctx, None).await?;
            Ok(Response::Agents(AgentsResponse::Accounts(overview)))
        }
        AgentsRequest::RemoveAccount { id } => {
            let mut settings = ctx.projects.agent_settings()?;
            accounts::remove(&mut settings, &id)?;
            ctx.projects.set_agent_settings(settings)?;
            let overview = accounts::overview(ctx, None).await?;
            Ok(Response::Agents(AgentsResponse::Accounts(overview)))
        }
        AgentsRequest::SetProjectAccount { project, account } => {
            ctx.projects.get(&project)?;
            let mut settings = ctx.projects.agent_settings()?;
            accounts::set_project(&mut settings, &project, account)?;
            ctx.projects.set_agent_settings(settings)?;
            Ok(OK)
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
        AgentsRequest::History { query, project } => {
            let names = ctx
                .projects
                .list()?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect();
            Ok(Response::Agents(AgentsResponse::History(sessions.history(
                &query,
                project.as_ref(),
                &names,
            ))))
        }
        AgentsRequest::Start {
            project,
            agent,
            mode,
            prompt,
            account,
            context,
            no_web,
        } => {
            if no_web && mode != SessionMode::Research {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "only a research session can be started without web search",
                ));
            }
            if mode == SessionMode::Isolated {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "isolated sessions are started by a coordinator (orchestration.start)",
                ));
            }
            let context = match sessions.core() {
                Some(core) => crate::context::message(&core, &project, None, &context).await?,
                None if context.is_empty() => None,
                None => return Err(shutting_down()),
            };
            let new = NewSession {
                project,
                agent,
                mode,
                prompt,
                context,
                cwd: None,
                tools: None,
                from: MessageFrom::Person,
                account,
                queued: false,
                preparing: false,
                no_web,
            };
            let info = start(ctx, new).await?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Resume { session, prompt } => {
            let info = resume(ctx, session, vec![(prompt, MessageFrom::Person)]).await?;
            Ok(Response::Agents(AgentsResponse::Session(info)))
        }
        AgentsRequest::Send {
            session,
            text,
            context,
        } => {
            // The person's context and picks go with their next message, in
            // Ovra's words.
            let core = sessions.core();
            let added = match &core {
                Some(core) => {
                    let project = sessions.find(&session, |s| s.info().project)?;
                    crate::context::message(core, &project, Some(&session), &context).await?
                }
                None if context.is_empty() => None,
                None => return Err(shutting_down()),
            };
            let picks = core
                .as_ref()
                .and_then(|c| crate::browser::pending(c, &session));
            let mut parts = vec![(text, MessageFrom::Person)];
            parts.extend(added.map(|t| (t, MessageFrom::Ovra)));
            parts.extend(picks.as_ref().map(|(t, _)| (t.clone(), MessageFrom::Ovra)));
            send(ctx, session.clone(), parts)?;
            if let (Some(core), Some((_, count))) = (&core, picks) {
                crate::browser::sent(core, &session, count);
            }
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
                &by,
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
            let (messages, seq) = sessions.find(&session, |s| (s.transcript(), s.seq()))?;
            Ok(Response::Agents(AgentsResponse::Transcript(
                SessionTranscript { messages, seq },
            )))
        }
        AgentsRequest::Capacity { project } => {
            ctx.projects.get(&project)?;
            let core = sessions.core().ok_or_else(|| {
                ProtocolError::new(ErrorCode::Unavailable, "Ovra is shutting down")
            })?;
            let capacity = crate::orchestration::capacity(&core, &project);
            Ok(Response::Agents(AgentsResponse::Capacity(capacity)))
        }
        AgentsRequest::Limit | AgentsRequest::SetLimit { .. } => {
            let core = sessions.core().ok_or_else(|| {
                ProtocolError::new(ErrorCode::Unavailable, "Ovra is shutting down")
            })?;
            let limit = match request {
                AgentsRequest::SetLimit { limit } => crate::orchestration::set_limit(&core, limit)?,
                _ => crate::orchestration::limit(&core),
            };
            Ok(Response::Agents(AgentsResponse::Limit(limit)))
        }
        AgentsRequest::ResearchLimit | AgentsRequest::SetResearchLimit { .. } => {
            let core = sessions.core().ok_or_else(|| {
                ProtocolError::new(ErrorCode::Unavailable, "Ovra is shutting down")
            })?;
            let limit = match request {
                AgentsRequest::SetResearchLimit { limit } => {
                    crate::orchestration::set_research_limit(&core, limit)?
                }
                _ => crate::orchestration::research_limit(&core),
            };
            Ok(Response::Agents(AgentsResponse::ResearchLimit(limit)))
        }
        AgentsRequest::Checkpoints { session } => {
            writes_main(ctx, &session)?;
            let list = sessions.direct.checkpoints(&session).await?;
            Ok(Response::Agents(AgentsResponse::Checkpoints(list)))
        }
        AgentsRequest::GoBack { session, turn } => {
            writes_main(ctx, &session)?;
            let (restored, conflicts) = sessions.direct.go_back(&session, turn).await?;
            let can_undo = !restored.is_empty();
            if can_undo {
                sessions.find(&session, |s| s.went_back(Some(turn), true))?;
            }
            Ok(Response::Agents(AgentsResponse::WentBack {
                restored,
                conflicts,
                can_undo,
            }))
        }
        AgentsRequest::UndoGoBack { session } => {
            writes_main(ctx, &session)?;
            let (restored, conflicts) = sessions.direct.undo_go_back(&session).await?;
            sessions.find(&session, |s| s.went_back(None, false))?;
            Ok(Response::Agents(AgentsResponse::UndidGoBack {
                restored,
                conflicts,
            }))
        }
    }
}

/// Checkpoints are kept for direct sessions that change the main checkout
/// (`docs/specs/checkpoints.md`).
fn writes_main(ctx: &Ctx<'_>, session: &SessionId) -> Result<()> {
    let (mode, launch) = ctx
        .sessions
        .find(session, |s| (s.info().mode, s.launch()))?;
    let refused = match (mode, launch.and_then(|l| l.tools)) {
        (SessionMode::Isolated, _) => {
            "going back to an earlier turn is for direct sessions; a worker's changes wait \
             for review in its own worktree"
        }
        (_, Some(ToolRole::Coordinator)) => "a coordinator does not change files",
        _ => return Ok(()),
    };
    Err(ProtocolError::new(ErrorCode::BadRequest, refused))
}

/// `parts` after the note a go-back left for the agent, if any.
fn with_note(ctx: &Ctx<'_>, session: &SessionId, parts: Parts) -> Parts {
    let note = ctx.sessions.direct.note(session);
    note.map(|n| (n, MessageFrom::Ovra))
        .into_iter()
        .chain(parts)
        .collect()
}

/// `parts` and then what waits for the agent on its task's board, so one
/// message starts the turn with the sender's words first. The entries stay
/// queued until [`delivered`]: a message that would be too long with them
/// goes without them.
fn with_board(
    ctx: &Ctx<'_>,
    session: &SessionId,
    parts: Parts,
) -> (Parts, Option<crate::orchestration::BoardWaiting>) {
    let waiting = ctx
        .sessions
        .core()
        .and_then(|core| crate::orchestration::board_waiting(&core, session));
    let Some(waiting) = waiting else {
        return (parts, None);
    };
    let mut with = parts.clone();
    with.push((waiting.text.clone(), MessageFrom::Ovra));
    if joined(&with).is_ok() {
        (with, Some(waiting))
    } else {
        (parts, None)
    }
}

/// The board entries a message carried reached the agent.
fn delivered(
    ctx: &Ctx<'_>,
    session: &SessionId,
    waiting: Option<crate::orchestration::BoardWaiting>,
) {
    if let (Some(core), Some(waiting)) = (ctx.sessions.core(), waiting) {
        crate::orchestration::board_delivered(&core, session, waiting);
    }
}

/// `parts`, now in the transcript, start a turn: direct mode takes its
/// before snapshot.
fn turn_started(ctx: &Ctx<'_>, session: &SessionId, parts: &Parts) {
    let Ok(turn) = ctx.sessions.find(session, |s| s.turn_started()) else {
        return;
    };
    let message = parts
        .iter()
        .find(|(_, from)| *from != MessageFrom::Ovra)
        .or(parts.first())
        .map(|(text, _)| title(text))
        .unwrap_or_default();
    if ctx.sessions.direct.turn_started(session, turn, message) {
        ctx.sessions.record_turns(session);
    }
}

/// A session to start.
pub(crate) struct NewSession {
    pub(crate) project: ProjectId,
    pub(crate) agent: AgentKind,
    pub(crate) mode: SessionMode,
    pub(crate) prompt: String,
    /// Ovra's message after the prompt: the person's `@` context.
    pub(crate) context: Option<String>,
    /// Where it works; `None` for the project root.
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) tools: Option<ToolRole>,
    /// Who wrote `prompt`.
    pub(crate) from: MessageFrom,
    /// The Claude account ([`crate::protocol::agents::Account::id`]);
    /// `None` uses the project's.
    pub(crate) account: Option<String>,
    /// A research session without `WebSearch`.
    pub(crate) no_web: bool,
    /// Waits `queued` for [`start_queued`] instead of starting now.
    pub(crate) queued: bool,
    /// Waits `preparing` while its worktree is set up, then
    /// [`start_queued`] starts it; overrides `queued`.
    pub(crate) preparing: bool,
}

/// What a session asks of its provider.
#[derive(Debug, Clone, Copy)]
struct Asked<'a> {
    agent: AgentKind,
    mode: SessionMode,
    tools: Option<ToolRole>,
    account: Option<&'a str>,
}

/// Refuses what `asked.agent`'s provider cannot do (ADR 0011): without an
/// enforced policy it works only in its own worktree, without IDE tools and
/// a read-only role it does not coordinate, and without accounts it runs
/// only with its own default sign-in.
fn check_can(ctx: &Ctx<'_>, asked: Asked<'_>) -> Result<()> {
    let Some(provider) = ctx
        .runtime
        .providers()
        .into_iter()
        .find(|p| p.id == asked.agent)
    else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            format!("Ovra cannot start {} here", program(asked.agent)),
        ));
    };
    let can = provider.capabilities;
    let name = &provider.name;
    let refused = |why: String| Err(ProtocolError::new(ErrorCode::Unavailable, why));
    match (asked.mode, asked.tools) {
        (_, Some(ToolRole::Coordinator)) if !(can.ide_tools && can.read_only_role) => {
            return refused(format!(
                "{name} cannot plan and split a task; choose One agent, own copy"
            ));
        }
        (SessionMode::Direct, Some(ToolRole::Coordinator)) => {}
        (SessionMode::Direct, _) if !can.enforced_policy => {
            return refused(format!(
                "{name} works only in its own copy of the project, because Ovra cannot enforce \
                 its rules there; choose One agent, own copy"
            ));
        }
        (SessionMode::Research, _) => {
            return refused(format!("{name} cannot run research sessions yet"));
        }
        _ => {}
    }
    if asked.account.is_some_and(|a| a != DEFAULT_ACCOUNT) && !can.accounts {
        return refused(format!(
            "{name} runs only with its own sign-in, not an account"
        ));
    }
    Ok(())
}

/// The program `agent` runs, as people type it.
fn program(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Gemini => "gemini",
        AgentKind::Codex => "codex-acp",
    }
}

/// Starts a session and returns it as it is now.
pub(crate) async fn start(ctx: &Ctx<'_>, new: NewSession) -> Result<SessionInfo> {
    check_text(&new.prompt)?;
    // Coordinators take no place, workers wait for one when asked to; a
    // person's start past the cap waits too (`docs/specs/review-check.md`),
    // a research session's is refused.
    let (place, queued) = match new.tools {
        None => match check_room(ctx, &new.project, new.mode)? {
            Room::Free(place) => (place, new.queued),
            Room::Full => (None, true),
        },
        Some(_) => (None, new.queued),
    };
    let target = prepare(
        ctx,
        None,
        &new.project,
        Asked {
            agent: new.agent,
            mode: new.mode,
            tools: new.tools,
            account: new.account.as_deref(),
        },
    )
    .await?;
    let launch = Launch {
        cwd: new
            .cwd
            .unwrap_or_else(|| PathBuf::from(&target.project.root)),
        tools: new.tools,
    };
    let id = ctx.sessions.next_id();
    let base = baseline(ctx, &new.project, &id, new.mode, &launch).await?;
    let mut parts = vec![(new.prompt.clone(), new.from)];
    parts.extend(new.context.map(|text| (text, MessageFrom::Ovra)));
    let waits = queued || new.preparing;
    let (status, first) = if new.preparing {
        (AgentStatus::Preparing, Some(parts.clone()))
    } else if queued {
        (AgentStatus::Queued, Some(parts.clone()))
    } else {
        (AgentStatus::Starting, None)
    };
    let info = SessionInfo {
        id: id.clone(),
        project: new.project.clone(),
        agent: new.agent,
        mode: new.mode,
        status: status.clone(),
        title: title(&new.prompt),
        usage: TurnUsage::default(),
        resumable: false,
        billing: None,
        sandbox: None,
        sandbox_reported: false,
        account: target.account.id.clone(),
        undone_from: None,
        can_undo_go_back: false,
        no_web: new.no_web,
        started_ms: Some(super::now_ms()),
        active_ms: Some(super::now_ms()),
    };
    // With its folder and role from the start, so that it counts in its
    // role, and its first message if it waits.
    ctx.sessions.insert(info, launch.clone(), first);
    drop(place);
    if waits {
        // Told before anything can start it.
        ctx.sink.emit(AgentsEvent::Status {
            session: id.clone(),
            project: new.project.clone(),
            status,
        });
        if let Some(core) = ctx.sessions.core() {
            crate::resources::wake(&core);
        }
    }
    review_from(ctx, &id, &launch, base).await;
    if waits {
        let info = ctx.sessions.find(&id, |s| s.info());
        // A place may have freed since the person's start found none; a
        // worker's spawn does this once its worktree is set up.
        if queued
            && new.tools.is_none()
            && let Some(core) = ctx.sessions.core()
        {
            crate::orchestration::start_queued(&core, &new.project);
        }
        return info;
    }
    let target = trusted_in(ctx, target, &launch.cwd).await;
    run(ctx, &id, target, launch, parts, None).await
}

/// Starts the waiting session `id`, which [`Sessions::take_waiting`] moved
/// to `starting`, with `parts`, its first message.
pub(crate) async fn start_queued(
    ctx: &Ctx<'_>,
    id: SessionId,
    parts: Parts,
) -> Result<SessionInfo> {
    let (info, launch) = ctx.sessions.find(&id, |s| (s.info(), s.launch()))?;
    let host = AgentHost::new(
        id.clone(),
        info.project.clone(),
        ctx.sessions.clone(),
        ctx.sink.clone(),
    );
    let tools = launch.as_ref().and_then(|l| l.tools);
    let account = info.account.as_deref().unwrap_or(DEFAULT_ACCOUNT);
    let prepared = prepare(
        ctx,
        Some(&id),
        &info.project,
        Asked {
            agent: info.agent,
            mode: info.mode,
            tools,
            account: Some(account),
        },
    )
    .await;
    let (target, launch) = match (prepared, launch) {
        (Ok(target), Some(launch)) => (target, launch),
        (Err(err), _) => {
            host.status(AgentStatus::Failed {
                message: err.message.clone(),
            });
            return Err(err);
        }
        (Ok(_), None) => {
            let err = conflict("the session has no folder to start in");
            host.status(AgentStatus::Failed {
                message: err.message.clone(),
            });
            return Err(err);
        }
    };
    let target = trusted_in(ctx, target, &launch.cwd).await;
    // Stopped while it was being prepared.
    if ctx.sessions.find(&id, |s| s.status().is_ended())? {
        return Err(conflict("the session was stopped before it started"));
    }
    run(ctx, &id, target, launch, parts, None).await
}

/// Takes a place for a person's start, or refuses it while the project has
/// as many agents working as it may: research sessions against their own
/// cap (ADR 0013). Hold the place until the session is in the list with its
/// status, where it counts itself.
fn check_room(ctx: &Ctx<'_>, project: &ProjectId, mode: SessionMode) -> Result<Room> {
    let Some(core) = ctx.sessions.core() else {
        return Ok(Room::Free(None));
    };
    if mode == SessionMode::Research {
        return crate::orchestration::reserve_research(&core, project)
            .map(|place| Room::Free(Some(place)))
            .map_err(|(working, limit)| {
                conflict(&match limit {
                    0 => "research agents are turned off in Settings".to_owned(),
                    _ => format!(
                        "{working} research agents are already working in this project, the \
                         most at once (Settings). Start this when one of them finishes, or stop one."
                    ),
                })
            });
    }
    Ok(match crate::orchestration::reserve(&core, project) {
        Some(place) => Room::Free(Some(place)),
        None => Room::Full,
    })
}

/// Whether a person's start found a place.
enum Room {
    /// Hold the place until the session is in the list with its status,
    /// where it counts itself; `None` without orchestration.
    Free(Option<crate::orchestration::Reservation>),
    /// The project has as many agents working as it may: the start waits.
    Full,
}

/// The project a session may start in, with what the runtime needs to know.
struct Target {
    project: Project,
    account: accounts::Chosen,
    trusted: bool,
}

/// `account`: the session's own ([`NewSession::account`]); a resume passes
/// the one it ran with, which must still exist. `id`: the session, if it is
/// in the list already (queued), so its own writer lock does not refuse it.
async fn prepare(
    ctx: &Ctx<'_>,
    id: Option<&SessionId>,
    project: &ProjectId,
    asked: Asked<'_>,
) -> Result<Target> {
    let Asked {
        agent,
        mode,
        tools,
        account,
    } = asked;
    let target = ctx.projects.get(project)?;
    let settings = ctx.projects.agent_settings()?;
    let account = match agent {
        AgentKind::Claude => accounts::choose(&settings, project, account)?,
        other => {
            check_can(ctx, asked)?;
            accounts::Chosen {
                id: None,
                executable: settings.path(other).map(str::to_owned),
                config_dir: None,
            }
        }
    };
    // Settings that changed since the user trusted them (or cannot be read)
    // do not load until the user reviews them again.
    let trusted = target.trusted && review(ctx, project).await.is_ok_and(|r| r.applies());
    // A coordinator only reads the main checkout, so it does not take the
    // direct writer's place (docs/specs/orchestration.md).
    let writes_main = mode == SessionMode::Direct && tools != Some(ToolRole::Coordinator);
    if writes_main && ctx.sessions.direct_writer(project, id) {
        return Err(conflict(
            "another agent already works in this project's main checkout",
        ));
    }
    Ok(Target {
        project: target,
        account,
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
async fn review_from(ctx: &Ctx<'_>, id: &SessionId, launch: &Launch, base: Option<String>) {
    if let (Some(base), Some(core)) = (base, ctx.sessions.core()) {
        crate::orchestration::direct::review_from(&core, id, launch.cwd.clone(), base).await;
    }
}

/// A message to an agent in parts, each with who wrote it: the agent gets
/// them as one text with each author marked (`provenance`), the transcript
/// shows each part with its author.
pub(crate) type Parts = Vec<(String, MessageFrom)>;

fn joined(parts: &Parts) -> Result<String> {
    let text = parts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    check_text(&text)?;
    let marked =
        crate::provenance::for_agent(parts.iter().map(|(text, from)| (text.as_str(), *from)));
    check_text(&marked)?;
    Ok(marked)
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
    let place = match tools {
        Some(ToolRole::Coordinator) => None,
        // A resume does not wait: the person picks the moment.
        _ => match check_room(ctx, &info.project, info.mode)? {
            Room::Free(place) => place,
            Room::Full => {
                return Err(conflict(
                    "every agent place in this project is taken. Continue this when one of \
                     them finishes, or stop one. Settings > Claude sets how many may work at once.",
                ));
            }
        },
    };
    // Its conversation lives in its account's folder: no other account
    // can continue it.
    let account = info.account.as_deref().unwrap_or(DEFAULT_ACCOUNT);
    if ctx
        .projects
        .agent_settings()?
        .account(Some(account))
        .is_none()
    {
        return Err(conflict(
            "this session's Claude account was removed; add it again to continue the session",
        ));
    }
    let target = prepare(
        ctx,
        None,
        &info.project,
        Asked {
            agent: info.agent,
            mode: info.mode,
            tools,
            account: Some(account),
        },
    )
    .await?;
    let launch = launch.unwrap_or(Launch {
        cwd: PathBuf::from(&target.project.root),
        tools: None,
    });
    let base = baseline(ctx, &info.project, &session, info.mode, &launch).await?;
    review_from(ctx, &session, &launch, base).await;
    let restart = || sessions.find(&session, |s| s.restart());
    match sessions.core() {
        // Checked together with the restart, so a worktree being removed
        // is never resumed into.
        Some(core) => crate::orchestration::unless_cleaned(&core, &session, restart)??,
        None => restart()?,
    }
    // Restarted: it counts itself now.
    drop(place);
    let target = trusted_in(ctx, target, &launch.cwd).await;
    run(ctx, &session, target, launch, parts, Some(agent_session)).await
}

/// What a session may touch in a sandbox. A worker reads its main
/// repository's git folder, which its worktree's history lives in.
/// `forward`: the ports of its work dir's servers (ADR 0008). The agent's
/// own state is the runtime's to add: it knows its provider's lists, and
/// the account's folder is `AgentSpec::config_dir`.
pub(crate) fn profile(
    project: &Project,
    mode: SessionMode,
    cwd: &Path,
    network: ProfileNetwork,
    forward: Vec<u16>,
) -> SandboxProfile {
    SandboxProfile {
        kind: crate::services::ProfileKind::Agent,
        agent_state: None,
        project: Some(project.id.clone()),
        work_env: None,
        work_dir: cwd.to_path_buf(),
        work_dir_read_only: mode == SessionMode::Research,
        read: match mode {
            SessionMode::Isolated => vec![PathBuf::from(&project.root).join(".git")],
            SessionMode::Direct | SessionMode::Research => Vec::new(),
        },
        write: Vec::new(),
        expose: Vec::new(),
        forward,
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
    let parts = with_note(ctx, id, parts);
    let (parts, board) = with_board(ctx, id, parts);
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
        crate::resources::wake(&core);
        crate::orchestration::on_status(&core, id, &AgentStatus::Starting);
    }
    let host = AgentHost::new(
        id.clone(),
        project.clone(),
        sessions.clone(),
        ctx.sink.clone(),
    );
    for (text, from) in parts.clone() {
        host.message(AgentMessage::UserText { text, from });
    }
    turn_started(ctx, id, &parts);
    let (agent, mode, no_web) = sessions.find(id, |s| {
        let info = s.info();
        (info.agent, info.mode, info.no_web)
    })?;
    let network = sessions
        .net_start(
            ctx.projects,
            ctx.secrets,
            &project,
            id,
            run,
            ctx.sink.clone(),
        )
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
    let core = sessions.core();
    // A research session has no tool that could reach a server.
    let forward = match &core {
        Some(core) if mode != SessionMode::Research => {
            crate::servers::forward(core, &launch.cwd).await
        }
        _ => Vec::new(),
    };
    if let Some(core) = &core {
        crate::servers::session_started(core, id);
    }
    let config_dir = target.account.config_dir.map(PathBuf::from);
    let mut sandbox = profile(&target.project, mode, &launch.cwd, network, forward);
    // A worker's ports and roots (`docs/specs/worktree-setup.md`).
    sandbox.work_env = core
        .as_ref()
        .and_then(|core| crate::orchestration::work_env(core, &launch.cwd));
    sessions.find(id, |s| s.set_network(sandbox.network.clone()))?;
    let spec = AgentSpec {
        session: id.clone(),
        agent,
        mode,
        cwd: launch.cwd,
        // A provider that cannot reach Ovra's IDE tools works without them
        // in its role (`docs/specs/more-agents.md`).
        tools: launch.tools.filter(|_| {
            agent == AgentKind::Claude
                || ctx
                    .runtime
                    .providers()
                    .iter()
                    .any(|p| p.id == agent && p.capabilities.ide_tools)
        }),
        prompt,
        executable: target.account.executable.map(PathBuf::from),
        config_dir,
        trusted: target.trusted,
        resume,
        sandbox,
        no_web,
    };
    match ctx.runtime.start(spec, host.clone()) {
        Ok(process) => {
            delivered(ctx, id, board);
            sessions.find(id, |s| s.set_process(Arc::from(process)))?;
        }
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

/// Sends `parts` as the running session's next message; empty `parts`
/// send only what waits for it on its task's board, if anything.
pub(crate) fn send(ctx: &Ctx<'_>, session: SessionId, parts: Parts) -> Result<()> {
    let only_board = parts.is_empty();
    if !only_board {
        joined(&parts)?;
    }
    let sessions = ctx.sessions;
    let (process, info, stopping) =
        sessions.find(&session, |s| (s.process(), s.info(), s.stopping()))?;
    let process = match process {
        Some(process) if !stopping => process,
        Some(_) => return Err(conflict("the session is stopping")),
        None if info.status == AgentStatus::Queued => {
            return Err(conflict(
                "the agent waits for a free place and starts when another agent finishes",
            ));
        }
        None if info.status == AgentStatus::Preparing => {
            return Err(conflict(
                "the agent's worktree is being prepared; it starts when that is done",
            ));
        }
        None => return Err(conflict("the session has ended")),
    };
    let parts = with_note(ctx, &session, parts);
    let (parts, board) = with_board(ctx, &session, parts);
    if only_board && board.is_none() {
        return Ok(());
    }
    let text = joined(&parts)?;
    let host = AgentHost::new(
        session.clone(),
        info.project,
        sessions.clone(),
        ctx.sink.clone(),
    );
    for (text, from) in parts.clone() {
        host.message(AgentMessage::UserText { text, from });
    }
    turn_started(ctx, &session, &parts);
    host.set_status(AgentStatus::Working);
    process.send(text)?;
    delivered(ctx, &session, board);
    Ok(())
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

/// `by`: who answers, for the audit log.
pub(crate) fn permissions(
    ctx: &Ctx<'_>,
    request: PermissionsRequest,
    by: DecidedBy,
) -> Result<Response> {
    let permissions = &ctx.sessions.permissions;
    match request {
        PermissionsRequest::Pending => Ok(Response::Permissions(PermissionsResponse::Pending(
            permissions.pending(),
        ))),
        PermissionsRequest::Answer { id, answer, .. } => {
            let decision = answer.decision;
            if !permissions.answer(&id, answer, by) {
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

fn shutting_down() -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, "Ovra is shutting down")
}
