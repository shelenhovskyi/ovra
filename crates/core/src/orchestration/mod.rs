//! Orchestration and review state (`docs/specs/orchestration.md`): tasks of
//! a coordinator and its workers, their limits, review items, the holds
//! running sessions keep on their projects, and the answers to Ovra's own
//! tools. Pure state: worktrees and agents are reached through services.
//!
//! Lock order: this state's lock may be held while taking the sessions'
//! lock, never the other way round; sessions call in here only without
//! holding theirs.

pub(crate) mod direct;
mod ide;
mod policy;
mod review;
mod saved;
mod slots;
mod tasks;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub use ide::{Output, WorkerState};
pub(crate) use policy::{IDE_TOOL_PREFIX, decide};

use crate::Core;
use crate::protocol::agents::{AgentStatus, SessionId};
use crate::protocol::orchestration::{OrchestrationEvent, OrchestrationRequest};
use crate::protocol::review::{ReviewEvent, ReviewRequest};
use crate::protocol::{ErrorCode, ProtocolError, Response};
use crate::services::SessionStore;

pub(crate) use saved::restore;
pub(crate) use slots::capacity;
pub(crate) use tasks::unless_cleaned;

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
    if *status == AgentStatus::Stopped {
        tasks::stop_workers_of(core, session);
    }
    match status {
        AgentStatus::Stopped => tasks::tell_coordinator(core, session, "stopped"),
        AgentStatus::Failed { message } => {
            tasks::tell_coordinator(core, session, &format!("failed: {message}"));
        }
        _ => {}
    }
    if !(status.is_ended() || *status == AgentStatus::Idle) {
        return;
    }
    // It may have made room for a queued worker.
    slots::start_queued(core, &project);
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
