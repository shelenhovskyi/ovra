//! One cap for the whole project (CLAUDE.md, ADR 0006 rules): at most
//! [`MAX_WORKING`] agents work at once, whichever task they belong to. A
//! worker beyond it waits `queued` and starts when a place frees; a person's
//! start beyond it is refused.

use super::{State, tasks};
use crate::Core;
use crate::agents;
use crate::protocol::agents::{AgentCapacity, AgentStatus};
use crate::protocol::projects::ProjectId;
use crate::services::ToolRole;

/// Agents that may work at once in one project.
pub(crate) const MAX_WORKING: usize = 3;

/// How many agents of `project` work and wait now.
pub(crate) fn capacity(core: &Core, project: &ProjectId) -> AgentCapacity {
    let state = core.orchestration_state();
    let guard = state.lock();
    let (working, queued) = count(core, &guard, project);
    let n = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    AgentCapacity {
        working: n(working),
        limit: n(MAX_WORKING),
        queued: n(queued),
    }
}

/// Agents of `project` that work, and workers that wait for a place.
/// Coordinators do not count: they only plan and wait for their workers,
/// and counting them would let three tasks block each other's workers for
/// good. A worker that asked for review and waits idle for the person does
/// no work either; a message wakes it again.
pub(super) fn count(core: &Core, state: &State, project: &ProjectId) -> (usize, usize) {
    let asked = tasks::asked_for_review(&state.reviews);
    let (mut working, mut queued) = (0, 0);
    for (id, status, role) in core.sessions().active(project) {
        if role == Some(ToolRole::Coordinator) {
            continue;
        }
        match status {
            AgentStatus::Queued => queued += 1,
            AgentStatus::Idle if asked.contains(&id) => {}
            _ => working += 1,
        }
    }
    for task in state.tasks.iter().filter(|t| &t.project == project) {
        let (wait, work) = task.starting();
        working += work;
        queued += wait;
    }
    (working, queued)
}

/// Starts queued workers of `project`, oldest first, while there is room.
/// Each was asked for by the coordinator of a task a person started.
pub(super) fn start_queued(core: &Core, project: &ProjectId) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    loop {
        // Counted and taken under one lock, so two calls never both take
        // the last place.
        let next = {
            let state = core.orchestration_state();
            let guard = state.lock();
            if count(core, &guard, project).0 >= MAX_WORKING {
                return;
            }
            core.sessions().dequeue(project)
        };
        let Some((id, parts)) = next else {
            return;
        };
        let core = core.clone();
        runtime.spawn(async move {
            // A start that fails shows as the session's failure.
            let _ = agents::start_queued(&core.agents_ctx(), id, parts).await;
        });
    }
}
