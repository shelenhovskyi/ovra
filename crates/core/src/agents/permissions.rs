//! Permission requests waiting for a person's answer.

use std::sync::Mutex;

use tokio::sync::oneshot;

use super::lock;
use crate::protocol::agents::SessionId;
use crate::protocol::permissions::{PermissionAnswer, PermissionId, PermissionRequest};

#[derive(Debug, Default)]
pub(crate) struct Permissions {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    asked: u64,
    pending: Vec<(PermissionRequest, oneshot::Sender<PermissionAnswer>)>,
}

impl Permissions {
    pub(crate) fn register(
        &self,
        session: SessionId,
        tool: String,
        input: serde_json::Value,
        title: Option<String>,
    ) -> (PermissionRequest, oneshot::Receiver<PermissionAnswer>) {
        let mut state = lock(&self.state);
        state.asked += 1;
        let request = PermissionRequest {
            id: PermissionId(format!("perm{}", state.asked)),
            session,
            tool,
            input,
            title,
        };
        let (tx, rx) = oneshot::channel();
        state.pending.push((request.clone(), tx));
        (request, rx)
    }

    pub(crate) fn pending(&self) -> Vec<PermissionRequest> {
        lock(&self.state)
            .pending
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }

    pub(crate) fn has_pending(&self, session: &SessionId) -> bool {
        lock(&self.state)
            .pending
            .iter()
            .any(|(r, _)| &r.session == session)
    }

    /// Passes the answer to the waiting agent. `false` if nothing waits for `id`.
    pub(crate) fn answer(&self, id: &PermissionId, answer: PermissionAnswer) -> bool {
        let mut state = lock(&self.state);
        let Some(index) = state.pending.iter().position(|(r, _)| &r.id == id) else {
            return false;
        };
        let (_, tx) = state.pending.remove(index);
        drop(state);
        // The agent may have stopped waiting; then the answer has no effect.
        let _ = tx.send(answer);
        true
    }

    /// Forgets every request of a session; their waiters see a closed channel.
    pub(crate) fn drop_session(&self, session: &SessionId) -> Vec<PermissionId> {
        let mut state = lock(&self.state);
        let (dropped, kept) = std::mem::take(&mut state.pending)
            .into_iter()
            .partition::<Vec<_>, _>(|(r, _)| &r.session == session);
        state.pending = kept;
        dropped.into_iter().map(|(r, _)| r.id).collect()
    }
}
