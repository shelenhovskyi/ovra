//! One cap for the whole project (CLAUDE.md, ADR 0006 rules): at most
//! [`limit`] agents work at once, whichever task they belong to. A
//! worker beyond it waits `queued` and starts when a place frees; a person's
//! start beyond it is refused. Both are checked and their place taken under
//! the one lock, so starts at the same moment never pass the cap together.
//! Research sessions (ADR 0011) have a cap of their own, from the user's
//! settings, and never count against the other.

use super::{State, tasks};
use crate::Core;
use crate::agents;
use crate::protocol::agents::{AgentCapacity, AgentLimit, AgentStatus, AgentsEvent, SessionMode};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, ProtocolError};
use crate::resources;
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
    let research = research_count(core, &guard, project);
    let n = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    AgentCapacity {
        working: n(working),
        limit: n(limit),
        queued: n(queued),
        research_working: n(research),
        research_limit: research_limit(core),
    }
}

/// Research sessions (ADR 0011) that may work at once in one project,
/// unless the person chose another number, from 0 (none) to
/// [`MAX_RESEARCH_LIMIT`]. They read and search only, so they get places
/// of their own, but each still uses the person's plan.
const DEFAULT_RESEARCH_LIMIT: u32 = 2;
const MIN_RESEARCH_LIMIT: u32 = 0;
const MAX_RESEARCH_LIMIT: u32 = 5;

/// The research cap now; an unreadable setting allows none.
fn research_limit(core: &Core) -> u32 {
    core.project_store().agent_settings().map_or(0, |settings| {
        settings
            .research_limit
            .filter(|n| (MIN_RESEARCH_LIMIT..=MAX_RESEARCH_LIMIT).contains(n))
            .unwrap_or(DEFAULT_RESEARCH_LIMIT)
    })
}

fn research_view_of(limit: u32) -> AgentLimit {
    AgentLimit {
        limit,
        default: DEFAULT_RESEARCH_LIMIT,
        min: MIN_RESEARCH_LIMIT,
        max: MAX_RESEARCH_LIMIT,
    }
}

/// The research cap and its range, as `agents.research_limit` answers.
pub(crate) fn research_view(core: &Core) -> AgentLimit {
    research_view_of(research_limit(core))
}

/// `agents.set_research_limit`: saves the person's choice. Under the
/// orchestration lock, so no research start counts against a cap half
/// changed.
pub(crate) fn set_research_limit(core: &Core, limit: u32) -> Result<AgentLimit, ProtocolError> {
    if !(MIN_RESEARCH_LIMIT..=MAX_RESEARCH_LIMIT).contains(&limit) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!(
                "the research limit must be between {MIN_RESEARCH_LIMIT} and {MAX_RESEARCH_LIMIT}"
            ),
        ));
    }
    {
        let state = core.orchestration_state();
        let _guard = state.lock();
        let store = core.project_store();
        let mut settings = store.agent_settings()?;
        settings.research_limit = (limit != DEFAULT_RESEARCH_LIMIT).then_some(limit);
        store.set_agent_settings(settings)?;
    }
    let view = research_view_of(limit);
    core.emit(AgentsEvent::ResearchLimitChanged(view));
    Ok(view)
}

/// Research sessions of `project` that work, and starts that took a place.
/// One that waits idle for the person does no work, like a person's own
/// direct session.
fn research_count(core: &Core, state: &State, project: &ProjectId) -> usize {
    let working = core
        .sessions()
        .active(project)
        .into_iter()
        .filter(|(_, status, _, mode)| {
            *mode == SessionMode::Research && *status != AgentStatus::Idle
        })
        .count();
    working + state.reserved_research.get(project).copied().unwrap_or(0)
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
    for (id, status, role, mode) in core.sessions().active(project) {
        if role == Some(ToolRole::Coordinator) || mode == SessionMode::Research {
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
    research: bool,
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
        research: false,
    })
}

/// Takes a research place in `project`, or says how many research
/// sessions work there (and the cap) if none is free.
pub(crate) fn reserve_research(
    core: &Core,
    project: &ProjectId,
) -> Result<Reservation, (usize, u32)> {
    let limit = research_limit(core);
    let state = core.orchestration_state();
    let mut guard = state.lock();
    let working = research_count(core, &guard, project);
    if working >= usize::try_from(limit).unwrap_or(usize::MAX) {
        return Err((working, limit));
    }
    *guard.reserved_research.entry(project.clone()).or_default() += 1;
    Ok(Reservation {
        core: core.clone(),
        project: project.clone(),
        research: true,
    })
}

impl Drop for Reservation {
    fn drop(&mut self) {
        {
            let state = self.core.orchestration_state();
            let mut guard = state.lock();
            let reserved = if self.research {
                &mut guard.reserved_research
            } else {
                &mut guard.reserved
            };
            if let Some(n) = reserved.get_mut(&self.project) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    reserved.remove(&self.project);
                }
            }
        }
        // A start that failed frees its place for a waiting worker.
        start_queued(&self.core, &self.project);
    }
}

/// Starts queued workers of `project`, oldest first, while there is room.
/// Each was asked for by the coordinator of a task a person started.
/// None starts while the computer is short of memory or CPU
/// ([`resources::holds_back`]).
pub(super) fn start_queued(core: &Core, project: &ProjectId) {
    if resources::holds_back(core) {
        return;
    }
    start(core, project, usize::MAX);
}

/// Starts the oldest queued worker of `project` if there is room: after a
/// shortage of memory or CPU ended, one per sample. Whether one started.
pub(crate) fn start_next_queued(core: &Core, project: &ProjectId) -> bool {
    start(core, project, 1) > 0
}

/// Starts up to `most` queued workers; how many started.
fn start(core: &Core, project: &ProjectId, most: usize) -> usize {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return 0;
    };
    let limit = limit(core);
    let mut started = 0;
    for _ in 0..most {
        // Counted and taken under one lock, so two calls never both take
        // the last place.
        let next = {
            let state = core.orchestration_state();
            let guard = state.lock();
            if count(core, &guard, project).0 >= limit {
                return started;
            }
            core.sessions().dequeue(project)
        };
        let Some((id, parts)) = next else {
            return started;
        };
        let core = core.clone();
        runtime.spawn(async move {
            // A start that fails shows as the session's failure.
            let _ = agents::start_queued(&core.agents_ctx(), id, parts).await;
        });
        started += 1;
    }
    started
}
