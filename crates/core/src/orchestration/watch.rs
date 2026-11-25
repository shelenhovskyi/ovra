//! Live review diffs: while a worker runs, a watch on its worktree turns
//! file changes into `review.changed` for the worker's latest item, so the
//! inbox shows its diff as it grows, not only when a turn ends.

use std::time::Duration;

use tokio::sync::broadcast;

use super::review;
use crate::Core;
use crate::protocol::Event;
use crate::protocol::agents::{SessionId, SessionMode};
use crate::protocol::fs::FsEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::review::ReviewEvent;
use crate::services::EventSink;

/// At most one `review.changed` per worker this often: each one makes the
/// inbox list and diff again, while an agent may write many times a second.
const EVERY: Duration = Duration::from_secs(1);

/// The id the file service knows a worker's watch by. Its events go only to
/// this module, never to clients.
fn id(session: &SessionId) -> ProjectId {
    ProjectId(format!("worktree:{}", session.0))
}

/// Starts watching `session`'s worktree, if it is a running worker with
/// one and not yet watched. Without a file watcher or a runtime nothing is
/// watched, and the diff refreshes at turn ends as before.
pub(super) fn start(core: &Core, session: &SessionId) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let Some(entry) = review::of_session(core, session) else {
        return;
    };
    if entry.mode != SessionMode::Isolated {
        return;
    }
    let state = core.orchestration_state();
    if !state.lock().watches.insert(session.clone()) {
        return;
    }
    // Checked after taking the place, so an end that comes in between
    // finds it and lets go.
    let ended = core
        .sessions()
        .find(session, |s| s.status().is_ended())
        .unwrap_or(true);
    if ended {
        state.lock().watches.remove(session);
        return;
    }
    let (tx, rx) = broadcast::channel(16);
    let watched = core
        .fs_service()
        .watch(id(session), entry.workspace.dir, EventSink(tx));
    if watched.is_err() {
        state.lock().watches.remove(session);
        return;
    }
    runtime.spawn(forward(core.downgrade(), session.clone(), rx));
}

/// Stops watching `session`'s worktree; it ended.
pub(super) fn stop(core: &Core, session: &SessionId) {
    if core.orchestration_state().lock().watches.remove(session) {
        core.fs_service().unwatch(&id(session));
    }
}

/// Emits `review.changed` for changes in the worktree, at most once per
/// [`EVERY`], until the watch ends or the core goes.
async fn forward(
    core: crate::core::WeakCore,
    session: SessionId,
    mut rx: broadcast::Receiver<Event>,
) {
    let mut due = false;
    loop {
        if !due {
            match rx.recv().await {
                Ok(event) if changes(&event) => {}
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
        let Some(core) = core.upgrade() else {
            return;
        };
        if let Some(entry) = review::of_session(&core, &session) {
            core.emit(ReviewEvent::Changed {
                project: entry.project,
                id: entry.id,
            });
        }
        drop(core);
        tokio::time::sleep(EVERY).await;
        due = false;
        loop {
            match rx.try_recv() {
                Ok(event) => due |= changes(&event),
                Err(broadcast::error::TryRecvError::Lagged(_)) => due = true,
                Err(broadcast::error::TryRecvError::Empty) => break,
                // The worker ended; its turn end refreshes the item.
                Err(broadcast::error::TryRecvError::Closed) => return,
            }
        }
    }
}

/// Whether `event` may change what the worker's diff shows.
fn changes(event: &Event) -> bool {
    matches!(
        event,
        Event::Fs(FsEvent::Changed { .. } | FsEvent::GitChanged { .. })
    )
}
