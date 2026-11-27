//! Orchestration and review state (`docs/specs/orchestration.md`): tasks of
//! a coordinator and its workers, their limits, review items, the holds
//! running sessions keep on their projects and workers on their worktrees,
//! and the answers to Ovra's own tools. Pure state: worktrees and agents
//! are reached through services.
//!
//! Lock order: this state's lock may be held while taking the sessions'
//! lock, never the other way round; sessions call in here only without
//! holding theirs.

mod behind;
mod board;
mod check;
pub(crate) mod direct;
mod hunks;
mod ide;
mod overlap;
mod policy;
mod prepare;
mod publish;
mod push;
mod review;
mod saved;
mod setup;
mod slots;
mod tasks;
mod warnings;
mod watch;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub(crate) use board::claimed_write;
pub use ide::{Output, WorkerState};
pub(crate) use policy::{IDE_TOOL_PREFIX, decide};
pub use push::{PushArgs, Pushed};

use crate::Core;
use crate::protocol::agents::{AgentStatus, SessionId};
use crate::protocol::orchestration::{OrchestrationEvent, OrchestrationRequest};
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{ReviewEvent, ReviewRequest};
use crate::protocol::{ErrorCode, ProtocolError, Response};
use crate::services::SessionStore;

pub(crate) use prepare::work_env;
pub(crate) use review::waits_for_review;
pub(crate) use saved::restore;
pub(crate) use slots::{
    Reservation, capacity, research_view as research_limit, reserve, reserve_research, set_limit,
    set_research_limit, start_next_queued, start_queued, view as limit,
};
pub(crate) use tasks::unless_cleaned;
pub(crate) use tasks::{Solo, start_solo};

type Result<T> = std::result::Result<T, ProtocolError>;

/// What a session's review compares: the working tree of `dir` against the
/// commit `base` (a worker's worktree and its start, or the main checkout
/// and a direct session's baseline).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Workspace {
    pub(crate) dir: PathBuf,
    pub(crate) base: String,
    /// `Worktrees::git_files` of `dir` when the session started, to show
    /// what changed in hooks and config (SEC-5); `None` if unknown.
    #[serde(default)]
    pub(crate) git: Option<BTreeMap<String, String>>,
}

#[derive(Debug)]
pub(crate) struct Orchestration {
    state: Mutex<State>,
    worktrees_dir: Option<PathBuf>,
    /// Keeps tasks and review items across restarts.
    store: Arc<dyn SessionStore>,
}

#[derive(Debug, Default)]
struct State {
    tasks: Vec<tasks::Entry>,
    tasks_made: u64,
    reviews: Vec<review::Entry>,
    reviews_made: u64,
    /// Sessions that hold their project open while they run.
    holds: BTreeSet<SessionId>,
    /// Workers whose worktree is watched while they run ([`watch`]).
    watches: BTreeSet<SessionId>,
    /// Each task's board ([`board`]).
    boards: BTreeMap<crate::protocol::orchestration::TaskId, board::Board>,
    /// Each worker's changed paths as [`overlap`] last saw them; not saved.
    changed: BTreeMap<SessionId, BTreeSet<String>>,
    /// Worker pairs [`overlap`] already told of; not saved.
    overlaps: BTreeSet<(SessionId, SessionId)>,
    /// (changer, claimer) pairs [`overlap`] told of a change to a claimed
    /// file; not saved.
    claim_overlaps: BTreeSet<(SessionId, SessionId)>,
    /// Places a person's starts took while their sessions are set up
    /// ([`slots::reserve`]); not saved.
    reserved: BTreeMap<ProjectId, usize>,
    /// How many agents may work at once ([`slots::limit`]), once read from
    /// the settings; not saved here.
    limit: Option<usize>,
    /// The same for research sessions ([`slots::reserve_research`]).
    reserved_research: BTreeMap<ProjectId, usize>,
    /// Workers whose worktree is being prepared ([`prepare`]).
    setups: BTreeMap<SessionId, tokio::task::AbortHandle>,
    /// Pull requests whose checks are polled ([`publish`]).
    polls: BTreeMap<crate::protocol::review::ReviewId, publish::Poll>,
    polls_made: u64,
    /// Each worker's latest check before review ([`check`]); not saved.
    checks: BTreeMap<SessionId, check::Run>,
    checks_made: u64,
}

impl Orchestration {
    /// The state as last saved in `store`; [`restore`] then checks it
    /// against the sessions that are kept.
    pub(crate) fn new(worktrees_dir: Option<PathBuf>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            state: Mutex::new(saved::load(store.as_ref())),
            worktrees_dir,
            store,
        }
    }

    /// Saves `state`; call it with the lock held, so saves keep their order.
    /// A failed save only means the change is not kept.
    fn save(&self, state: &State) {
        saved::save(self.store.as_ref(), state);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) async fn review(core: &Core, request: ReviewRequest) -> Result<Response> {
    review::handle(core, request).await
}

pub(crate) async fn pull_request(
    core: &Core,
    request: crate::protocol::pr::PrRequest,
) -> Result<Response> {
    publish::handle(core, request).await
}

pub(crate) use board::Waiting as BoardWaiting;

/// What waits for `session` on its task's board, as the text that goes
/// with the message starting its next turn; it stays queued until
/// [`board_delivered`].
pub(crate) fn board_waiting(core: &Core, session: &SessionId) -> Option<BoardWaiting> {
    board::waiting(core, session)
}

/// The message carrying `waiting` reached `session`: off the queue.
pub(crate) fn board_delivered(core: &Core, session: &SessionId, waiting: BoardWaiting) {
    board::delivered(core, session, waiting);
}

pub(crate) fn board(
    core: &Core,
    request: crate::protocol::board::BoardRequest,
) -> Result<Response> {
    board::handle(core, request)
}

pub(crate) async fn orchestration(core: &Core, request: OrchestrationRequest) -> Result<Response> {
    tasks::handle(core, request).await
}

/// Called whenever a session changes state (and with `starting` when it
/// starts or resumes). A running session holds its project like an open
/// window does, so its review diff stays live; the end lets go.
pub(crate) fn on_status(core: &Core, session: &SessionId, status: &AgentStatus) {
    let Ok(project) = core.sessions().find(session, |s| s.info().project) else {
        return;
    };
    check::on_status(core, session, status);
    let state = core.orchestration_state();
    if *status == AgentStatus::Starting {
        tasks::set_ended(core, session, false);
        if !state.lock().holds.contains(session) {
            // Without a watcher (a front end without file access) the
            // session simply holds nothing.
            if let Ok(root) = core.root(&project)
                && core.acquire(&project, root).is_ok()
            {
                state.lock().holds.insert(session.clone());
            }
        }
        // A resumed worker; a new one starts its watch once spawned.
        watch::start(core, session);
        return;
    }
    // Only a stop someone asked for ends a task: a process that exits on
    // its own, or because Ovra itself shuts down, may be resumed.
    let asked_to_stop = core
        .sessions()
        .find(session, |s| s.stopping())
        .unwrap_or(false);
    if status.is_ended() && asked_to_stop {
        tasks::set_ended(core, session, true);
    }
    if status.is_ended() && state.lock().holds.remove(session) {
        core.release(&project);
    }
    if status.is_ended() {
        watch::stop(core, session);
        prepare::abort(core, session);
    }
    if *status == AgentStatus::Stopped {
        tasks::stop_workers_of(core, session);
    }
    match status {
        AgentStatus::Stopped => tasks::tell_coordinator(core, session, "stopped"),
        AgentStatus::Failed { message } => {
            // The runtime's error may quote the CLI or a hook: one short
            // line that cannot open a tag, as the person reads it too.
            let line: String = message
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect();
            let message = crate::provenance::escaped(&line);
            tasks::tell_coordinator(core, session, &format!("failed: {message}"));
        }
        _ => {}
    }
    if !(status.is_ended() || *status == AgentStatus::Idle) {
        return;
    }
    // It may have made room for a queued worker.
    slots::start_queued(core, &project);
    // A finished turn may have changed files another worker changed too.
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let (core, session) = (core.clone(), session.clone());
        runtime.spawn(async move { overlap::check(&core, &session).await });
    }
    if *status == AgentStatus::Idle {
        prepare::count_disk(core, session);
    }
    // A finished turn or session may have changed files or spent money.
    let (review, task) = {
        let state = state.lock();
        let review = state
            .reviews
            .iter()
            .rev()
            .find(|r| &r.session == session)
            .map(|r| r.id.clone());
        let task = state
            .tasks
            .iter()
            .find(|t| t.has_session(session))
            .map(|t| t.id.clone());
        (review, task)
    };
    if let Some(id) = review {
        core.emit(ReviewEvent::Changed {
            project: project.clone(),
            id,
        });
    }
    if let Some(task) = task {
        core.emit(OrchestrationEvent::Changed { project, task });
    }
}

fn not_found(what: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("{what} not found"))
}

fn conflict(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}

fn forbidden(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Forbidden, message)
}

#[cfg(test)]
mod tests;
