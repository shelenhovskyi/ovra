//! Tasks and review items across restarts, as one JSON document in the
//! [`SessionStore`]. Sessions are kept by the store already; this keeps
//! which task each belongs to and what its review compares.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{State, board, review, tasks};
use crate::Core;
use crate::agents::Launch;
use crate::protocol::agents::SessionId;
use crate::protocol::orchestration::TaskId;
use crate::services::{SessionStore, ToolRole};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    tasks: Vec<tasks::Entry>,
    tasks_made: u64,
    reviews: Vec<review::Entry>,
    reviews_made: u64,
    boards: BTreeMap<TaskId, board::Board>,
}

/// What was saved; nothing if it is missing or cannot be read.
pub(super) fn load(store: &dyn SessionStore) -> State {
    let saved: Saved = store
        .load_orchestration()
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    State {
        tasks: saved.tasks,
        tasks_made: saved.tasks_made,
        reviews: saved.reviews,
        reviews_made: saved.reviews_made,
        boards: saved.boards,
        ..State::default()
    }
}

pub(super) fn save(store: &dyn SessionStore, state: &State) {
    #[derive(Serialize)]
    struct Ref<'a> {
        tasks: &'a [tasks::Entry],
        tasks_made: u64,
        reviews: &'a [review::Entry],
        reviews_made: u64,
        boards: &'a BTreeMap<TaskId, board::Board>,
    }
    let saved = Ref {
        tasks: &state.tasks,
        tasks_made: state.tasks_made,
        reviews: &state.reviews,
        reviews_made: state.reviews_made,
        boards: &state.boards,
    };
    if let Ok(json) = serde_json::to_string(&saved) {
        let _ = store.save_orchestration(&json);
    }
}

/// Matches the loaded state with the kept sessions: drops what belongs to
/// sessions that are gone (with their project, or forgotten as old), and
/// gives the task's sessions their role and folder back, so that resuming
/// a worker continues in its worktree and a coordinator keeps its tools.
pub(crate) fn restore(core: &Core) {
    let kept = |id: &SessionId| core.sessions().find(id, |_| ()).is_ok();
    let orchestration = core.orchestration_state();
    let mut state = orchestration.lock();
    state.reviews.retain(|r| kept(&r.session));
    state.tasks.retain(|t| kept(&t.coordinator));
    let tasks: Vec<_> = state.tasks.iter().map(|t| t.id.clone()).collect();
    state.boards.retain(|task, _| tasks.contains(task));
    for task in &mut state.tasks {
        task.workers.retain(|w| kept(&w.session));
    }
    for task in &state.tasks {
        if let Ok(project) = core.project_store().get(&task.project) {
            relaunch(
                core,
                &task.coordinator,
                project.root.into(),
                ToolRole::Coordinator,
            );
        }
        for worker in &task.workers {
            relaunch(
                core,
                &worker.session,
                worker.worktree.clone().into(),
                ToolRole::Worker,
            );
        }
    }
    orchestration.save(&state);
}

fn relaunch(core: &Core, session: &SessionId, cwd: PathBuf, role: ToolRole) {
    let _ = core.sessions().find(session, |s| {
        if s.launch().is_none() {
            s.set_launch(Launch {
                cwd,
                tools: Some(role),
            });
        }
    });
}
