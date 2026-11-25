//! Two workers of a task changing the same file
//! (`docs/research/agent-messaging.md`, recommendation 2). Worktrees keep
//! them from overwriting each other, so without this the overlap shows only
//! when the person approves the second one. The core sees every worker's
//! changes, so it tells the coordinator as soon as two workers' changes
//! meet, and tells each of the two that is still working, without waking
//! one that waits. Each worker pair and file is told once.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{review, tasks};
use crate::Core;
use crate::agents;
use crate::protocol::agents::{AgentStatus, MessageFrom, SessionId, SessionMode};
use crate::protocol::review::ReviewState;

/// Notes name at most this many files.
const NAMED: usize = 10;

/// Looks at `session`'s changed files again and tells of new overlaps with
/// the other workers of its task. Called when its worktree changes and when
/// its turn ends. Another worker's files are those its own last look saw,
/// or are read now if it has none yet.
pub(super) async fn check(core: &Core, session: &SessionId) {
    let Some(task) = tasks::of_worker(core, session) else {
        return;
    };
    let Some(mine) = changed(core, session).await else {
        return;
    };
    let mut others = Vec::new();
    for worker in task.workers.iter().filter(|w| &w.session != session) {
        let known = core
            .orchestration_state()
            .lock()
            .changed
            .contains_key(&worker.session);
        if !known && let Some(files) = changed(core, &worker.session).await {
            remember(core, &worker.session, files);
        }
        others.push(worker.clone());
    }
    // One look under one lock: of two checks that run at once, the later
    // one sees what the earlier one wrote.
    let found: Vec<(String, Vec<String>, SessionId)> = {
        let state = core.orchestration_state();
        let mut state = state.lock();
        state.changed.insert(session.clone(), mine.clone());
        let mut found = Vec::new();
        for other in &others {
            let Some(theirs) = state.changed.get(&other.session) else {
                continue;
            };
            let pair = ordered(session, &other.session);
            let new: Vec<String> = mine
                .intersection(theirs)
                .filter(|path| !state.overlaps.contains(&(pair.clone(), (*path).clone())))
                .cloned()
                .collect();
            if new.is_empty() {
                continue;
            }
            for path in &new {
                state.overlaps.insert((pair.clone(), path.clone()));
            }
            found.push((other.name.clone(), new, other.session.clone()));
        }
        found
    };
    let Some(me) = task.workers.iter().find(|w| &w.session == session) else {
        return;
    };
    let ctx = core.agents_ctx();
    for (other, paths, other_id) in found {
        let files = named(&paths);
        let coordinator_running = core
            .sessions()
            .find(&task.coordinator, |s| !s.status().is_ended())
            .unwrap_or(false);
        if coordinator_running {
            let note = format!(
                "Ovra: Workers {} and {other} both changed {files}. Only one version of \
                 each file can be kept without a conflict.\nFor the coordinator: their \
                 agent_ids are {} and {}. Tell one of them to leave these files, or split \
                 the work differently.",
                me.name, session.0, other_id.0
            );
            // A coordinator that is stopping simply misses the note.
            let _ = agents::send(
                &ctx,
                task.coordinator.clone(),
                vec![(note, MessageFrom::Ovra)],
            );
        }
        // Waking a worker that waits would start a turn it did not ask
        // for; the coordinator decides about that one.
        for (worker, other) in [(session, other.as_str()), (&other_id, me.name.as_str())] {
            if working(core, worker) {
                let note = format!(
                    "Ovra: Worker {other} of your task also changed {files}. Leave these \
                     files to it unless your subtask needs them; the coordinator knows."
                );
                let _ = agents::send(&ctx, worker.clone(), vec![(note, MessageFrom::Ovra)]);
            }
        }
    }
}

fn working(core: &Core, session: &SessionId) -> bool {
    core.sessions()
        .find(session, |s| {
            matches!(
                s.status(),
                AgentStatus::Working | AgentStatus::WaitingForApproval
            )
        })
        .unwrap_or(false)
}

/// The paths a worker's latest review item changes, old paths of renames
/// included; none once the person discarded it. `None` for a session that
/// is not a worker or whose files cannot be read now.
async fn changed(core: &Core, session: &SessionId) -> Option<BTreeSet<String>> {
    let entry = review::of_session(core, session)?;
    if entry.mode != SessionMode::Isolated {
        return None;
    }
    if entry.state == ReviewState::Rejected {
        return Some(BTreeSet::new());
    }
    let files = review::files(core, &entry).await.ok()?;
    Some(
        files
            .into_iter()
            .flat_map(|f| std::iter::once(f.path).chain(f.from))
            .collect(),
    )
}

fn remember(core: &Core, session: &SessionId, files: BTreeSet<String>) {
    core.orchestration_state()
        .lock()
        .changed
        .insert(session.clone(), files);
}

fn ordered(a: &SessionId, b: &SessionId) -> (SessionId, SessionId) {
    if a <= b {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    }
}

/// "a.rs, b.rs and 3 more files".
fn named(paths: &[String]) -> String {
    let mut text = paths
        .iter()
        .take(NAMED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > NAMED {
        let _ = write!(text, " and {} more files", paths.len() - NAMED);
    }
    text
}
