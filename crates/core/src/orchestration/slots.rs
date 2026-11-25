//! One cap for the whole project (CLAUDE.md, ADR 0006 rules): at most
//! [`limit`] agents work at once, whichever task they belong to. A
//! worker beyond it waits `queued` and starts when a place frees; a person's
//! start beyond it is refused. Both are checked and their place taken under
//! the one lock, so starts at the same moment never pass the cap together.

use super::{State, tasks};
use crate::Core;
use crate::agents;
use crate::protocol::agents::{AgentCapacity, AgentLimit, AgentStatus, AgentsEvent};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::ToolRole;

/// Agents that may work at once in one project, unless the person chose
/// another number in Settings, from [`MIN_LIMIT`] to [`MAX_LIMIT`].
const DEFAULT_LIMIT: u32 = 3;
const MIN_LIMIT: u32 = 1;
/// Each agent is a `claude` process with its own worktree, using the
/// person's plan limits; past this many, reviews pile up faster than anyone
/// reads them.
const MAX_LIMIT: u32 = 8;

/// How many agents may work at once in one project now.
pub(crate) fn limit(core: &Core) -> usize {
    let state = core.orchestration_state();
    if let Some(limit) = state.lock().limit {
        return limit;
    }
    let as_usize = |n: u32| usize::try_from(n).unwrap_or(1);
    // A failed read gives the default this once and is tried again.
    let Ok(settings) = core.project_store().agent_settings() else {
        return as_usize(DEFAULT_LIMIT);
    };
    let chosen = settings
        .max_working
        .filter(|n| (MIN_LIMIT..=MAX_LIMIT).contains(n))
        .unwrap_or(DEFAULT_LIMIT);
    // A `set_limit` that ran meanwhile wins over what was read before it.
    *state.lock().limit.get_or_insert(as_usize(chosen))
}

/// The limit and its range, as `agents.limit` answers.
pub(crate) fn view(core: &Core) -> AgentLimit {
    AgentLimit {
        limit: u32::try_from(limit(core)).unwrap_or(DEFAULT_LIMIT),
        default: DEFAULT_LIMIT,
        min: MIN_LIMIT,
        max: MAX_LIMIT,
    }
}

/// `agents.set_limit`: saves the person's choice; a higher limit starts
/// queued workers in every project.
pub(crate) fn set_limit(core: &Core, limit: u32) -> Result<AgentLimit, ProtocolError> {
    if !(MIN_LIMIT..=MAX_LIMIT).contains(&limit) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("the limit must be between {MIN_LIMIT} and {MAX_LIMIT}"),
        ));
    }
    let store = core.project_store();
    {
        // Saved and cached under one lock, so the store and the cap agree
        // when two changes come at once.
        let state = core.orchestration_state();
        let mut guard = state.lock();
        let mut settings = store.agent_settings()?;
        settings.max_working = (limit != DEFAULT_LIMIT).then_some(limit);
        store.set_agent_settings(settings)?;
        guard.limit = usize::try_from(limit).ok();
    }
    let view = view(core);
    core.emit(AgentsEvent::LimitChanged(view));
    // Done either way; a project list that fails only leaves workers
    // waiting until the next place frees.
    for project in store.list().unwrap_or_default() {
        start_queued(core, &project.id);
    }
    Ok(view)
}

/// How many agents of `project` work and wait now.
pub(crate) fn capacity(core: &Core, project: &ProjectId) -> AgentCapacity {
    let limit = limit(core);
    let state = core.orchestration_state();
    let guard = state.lock();
    let (working, queued) = count(core, &guard, project);
    let n = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    AgentCapacity {
        working: n(working),
        limit: n(limit),
        queued: n(queued),
    }
}

/// Agents of `project` that work, and workers that wait for a place.
/// Coordinators do not count: they only plan and wait for their workers,
/// and counting them would let three tasks block each other's workers for
/// good. A worker that asked for review and waits idle for the person does
/// no work either, nor does a person's own session that waits idle for
/// their reply; a message wakes either again, even at the cap.
pub(super) fn count(core: &Core, state: &State, project: &ProjectId) -> (usize, usize) {
    let asked = tasks::asked_for_review(&state.reviews);
    let (mut working, mut queued) = (0, 0);
    for (id, status, role) in core.sessions().active(project) {
        if role == Some(ToolRole::Coordinator) {
            continue;
        }
        match status {
            AgentStatus::Queued => queued += 1,
            AgentStatus::Idle if role.is_none() || asked.contains(&id) => {}
            _ => working += 1,
        }
    }
    working += state.reserved.get(project).copied().unwrap_or(0);
    for task in state.tasks.iter().filter(|t| &t.project == project) {
        let (wait, work) = task.starting();
        working += work;
        queued += wait;
    }
    (working, queued)
}

/// A place a person's start took while its session is set up; it lets go
/// when dropped, by then the session counts itself (or it failed).
#[derive(Debug)]
pub(crate) struct Reservation {
    core: Core,
    project: ProjectId,
}

/// Takes a place in `project` for a person's start, or says how many
/// agents work there if none is free.
pub(crate) fn reserve(core: &Core, project: &ProjectId) -> Result<Reservation, usize> {
    let limit = limit(core);
    let state = core.orchestration_state();
    let mut guard = state.lock();
    let working = count(core, &guard, project).0;
    if working >= limit {
        return Err(working);
    }
    *guard.reserved.entry(project.clone()).or_default() += 1;
    Ok(Reservation {
        core: core.clone(),
        project: project.clone(),
    })
}

impl Drop for Reservation {
    fn drop(&mut self) {
        {
            let state = self.core.orchestration_state();
            let mut guard = state.lock();
            if let Some(n) = guard.reserved.get_mut(&self.project) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    guard.reserved.remove(&self.project);
                }
            }
        }
        // A start that failed frees its place for a waiting worker.
        start_queued(&self.core, &self.project);
    }
}

/// Starts queued workers of `project`, oldest first, while there is room.
/// Each was asked for by the coordinator of a task a person started.
pub(super) fn start_queued(core: &Core, project: &ProjectId) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let limit = limit(core);
    loop {
        // Counted and taken under one lock, so two calls never both take
        // the last place.
        let next = {
            let state = core.orchestration_state();
            let guard = state.lock();
            if count(core, &guard, project).0 >= limit {
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
