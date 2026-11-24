//! Orchestration and review state (`docs/specs/orchestration.md`): tasks,
//! roles, limits and review items, and the holds running sessions keep on
//! their projects. Tasks and review items are stubs until the next commits.
//!
//! Lock order: this state's lock may be held while taking the sessions'
//! lock, never the other way round; sessions call in here only without
//! holding theirs.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::Core;
use crate::protocol::agents::{AgentStatus, SessionId};
use crate::protocol::orchestration::OrchestrationRequest;
use crate::protocol::review::ReviewRequest;
use crate::protocol::{ErrorCode, ProtocolError, Response};

#[derive(Debug, Default)]
pub(crate) struct Orchestration {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Sessions that hold their project open while they run.
    holds: BTreeSet<SessionId>,
}

impl Orchestration {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_yet(what: &str, task: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        format!("{what} is not implemented yet (task {task})"),
    )
}

pub(crate) fn review(_: ReviewRequest) -> Result<Response, ProtocolError> {
    Err(not_yet("review", "3.B"))
}

pub(crate) fn orchestration(_: OrchestrationRequest) -> Result<Response, ProtocolError> {
    Err(not_yet("orchestration", "3.B"))
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
    if status.is_ended() && state.lock().holds.remove(session) {
        core.release(&project);
    }
}

#[cfg(test)]
mod tests;
