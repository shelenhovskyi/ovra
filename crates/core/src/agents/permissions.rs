//! Permission requests waiting for a person's answer.

use std::sync::Mutex;

use tokio::sync::oneshot;

use super::lock;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::DecidedBy;
use crate::protocol::permissions::{PermissionAnswer, PermissionId, PermissionRequest};

#[derive(Debug, Default)]
pub(crate) struct Permissions {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    asked: u64,
    pending: Vec<Pending>,
}

/// A request and where its answer goes, with who gave it.
type Pending = (
    PermissionRequest,
    oneshot::Sender<(PermissionAnswer, DecidedBy)>,
);

impl Permissions {
    /// Gives `request` its id and keeps it until it is answered.
    pub(crate) fn register(
        &self,
        mut request: PermissionRequest,
    ) -> (
        PermissionRequest,
        oneshot::Receiver<(PermissionAnswer, DecidedBy)>,
    ) {
        let mut state = lock(&self.state);
        state.asked += 1;
        request.id = PermissionId(format!("perm{}", state.asked));
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

    /// Passes the person's answer to the waiting agent. `false` if nothing
    /// waits for `id`.
    pub(crate) fn answer(
        &self,
        id: &PermissionId,
        answer: PermissionAnswer,
        by: DecidedBy,
    ) -> bool {
        let mut state = lock(&self.state);
        let Some(index) = state.pending.iter().position(|(r, _)| &r.id == id) else {
            return false;
        };
        let (_, tx) = state.pending.remove(index);
        drop(state);
        // The agent may have stopped waiting; then the answer has no effect.
        let _ = tx.send((answer, by));
        true
    }

    /// Takes the request `id` out of the pending ones, so nobody else can
    /// answer it; `None` if nothing waits for it. Answer it with
    /// [`Taken::answer`], or [`Permissions::put_back`] it.
    pub(crate) fn take(&self, id: &PermissionId) -> Option<Taken> {
        let mut state = lock(&self.state);
        let index = state.pending.iter().position(|(r, _)| &r.id == id)?;
        let (request, tx) = state.pending.remove(index);
        Some(Taken {
            request,
            tx,
            at: index,
        })
    }

    /// Makes a taken request pending again, in its place, while its
    /// session is `live`; otherwise denies it, as an ended session's
    /// requests are. `true` if it is pending again: not when denied, nor
    /// when its agent stopped waiting meanwhile.
    pub(crate) fn put_back(&self, taken: Taken, live: bool) -> bool {
        if !live {
            let ended = PermissionAnswer::deny("the session ended");
            let _ = taken.tx.send((ended, DecidedBy::SessionEnded));
            return false;
        }
        if taken.tx.is_closed() {
            return false;
        }
        let mut state = lock(&self.state);
        let at = taken.at.min(state.pending.len());
        state.pending.insert(at, (taken.request, taken.tx));
        true
    }

    /// Removes a request nobody waits for any more. `false` if it was not pending.
    pub(crate) fn withdraw(&self, id: &PermissionId) -> bool {
        let mut state = lock(&self.state);
        let before = state.pending.len();
        state.pending.retain(|(r, _)| &r.id != id);
        state.pending.len() != before
    }

    /// Answers every request of a session with `answer`, given `by`.
    pub(crate) fn deny_session(
        &self,
        session: &SessionId,
        answer: &PermissionAnswer,
        by: &DecidedBy,
    ) -> Vec<PermissionId> {
        let requests = self.drop_requests(session);
        requests
            .into_iter()
            .map(|(request, tx)| {
                let _ = tx.send((answer.clone(), by.clone()));
                request.id
            })
            .collect()
    }

    /// Forgets every request of a session; their waiters see a closed channel.
    pub(crate) fn drop_session(&self, session: &SessionId) -> Vec<PermissionId> {
        self.drop_requests(session)
            .into_iter()
            .map(|(request, _)| request.id)
            .collect()
    }

    fn drop_requests(&self, session: &SessionId) -> Vec<Pending> {
        let mut state = lock(&self.state);
        let (dropped, kept) = std::mem::take(&mut state.pending)
            .into_iter()
            .partition::<Vec<_>, _>(|(r, _)| &r.session == session);
        state.pending = kept;
        dropped
    }
}

/// A request taken out of the pending ones, still waiting for its answer.
#[derive(Debug)]
pub(crate) struct Taken {
    pub(crate) request: PermissionRequest,
    tx: oneshot::Sender<(PermissionAnswer, DecidedBy)>,
    /// Its place among the pending requests.
    at: usize,
}

impl Taken {
    /// Passes the answer of the person at the computer ("Always allow" is
    /// never a phone's) to the waiting agent.
    pub(crate) fn answer(self, answer: PermissionAnswer) {
        // The agent may have stopped waiting; then the answer has no effect.
        let _ = self.tx.send((answer, DecidedBy::Person));
    }
}
