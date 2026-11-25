//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

mod input;

use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::Poll;
use std::time::Duration;

use tokio::sync::{Notify, oneshot};
use tokio::time::Instant;

use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::{
    AuditEvent, AuditLogEvent, AuditRange, AuditRequest, AuditResponse, DecidedBy, MAX_AUDIT_PAGE,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::SecretValue;
use crate::protocol::{ProtocolError, Response};

/// Appends `event` and shows it to live viewers. A log that cannot be
/// written does not stop the work it records, as with the session store;
/// it logs a warning that names the kind of entry, never what it holds.
pub(crate) fn record(
    core: &Core,
    project: &ProjectId,
    session: Option<&SessionId>,
    event: AuditEvent,
) {
    let kind = kind(&event);
    match core.audit_log().append(project, session, event) {
        Ok(entry) => core.emit(AuditLogEvent::Appended(entry)),
        Err(error) => tracing::warn!(
            project = %project.0,
            session = session.map(|s| s.0.as_str()),
            kind,
            code = ?error.code,
            "an audit entry could not be written: {}",
            error.message
        ),
    }
}

fn kind(event: &AuditEvent) -> &'static str {
    match event {
        AuditEvent::SessionStarted { .. } => "session_started",
        AuditEvent::ToolDecided { .. } => "tool_decided",
        AuditEvent::TrustChanged { .. } => "trust_changed",
        AuditEvent::PolicyChanged { .. } => "policy_changed",
        AuditEvent::ReviewDecided { .. } => "review_decided",
        AuditEvent::NetworkRefused { .. } => "network_refused",
        AuditEvent::SecretUsed { .. } => "secret_used",
        AuditEvent::ServerStarted { .. } => "server_started",
        AuditEvent::ServerStopped { .. } => "server_stopped",
        AuditEvent::LoopbackGranted { .. } => "loopback_granted",
        AuditEvent::LoopbackRevoked { .. } => "loopback_revoked",
    }
}

/// How long a decision waits for a keychain read before it is kept as if
/// the secrets could not be read. A late answer still fills the cache.
pub(crate) const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long after a failed read the project's decisions are kept as if the
/// secrets could not be read, without asking the keychain again.
pub(crate) const RETRY_AFTER: Duration = Duration::from_secs(10);

/// The core's audit state: tool decisions waiting to be recorded, in the
/// order each project's were made, and the secret values read for redaction.
#[derive(Default)]
pub(crate) struct State {
    queue: Mutex<HashMap<ProjectId, Lane>>,
    known: Arc<Mutex<Known>>,
}

/// Never shows a decision's input or a secret's value.
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("audit::State").finish_non_exhaustive()
    }
}

/// One project's decisions waiting to be recorded. A lane exists while a
/// task drains it; `more` wakes that task when a decision joins.
#[derive(Default)]
struct Lane {
    pending: VecDeque<Decided>,
    more: Arc<Notify>,
}

struct Decided {
    session: Option<SessionId>,
    tool: String,
    input: serde_json::Value,
    allowed: bool,
    by: DecidedBy,
    done: oneshot::Sender<()>,
}

/// Each project's secret values, as last read. `generation` changes with
/// every change to any project's secrets, so a read that began before one
/// is not kept. A project has at most one read in flight (`reading`), and
/// `failed` holds when its last read failed.
#[derive(Default)]
struct Known {
    values: HashMap<ProjectId, Arc<[SecretValue]>>,
    generation: u64,
    reading: HashSet<ProjectId>,
    failed: HashMap<ProjectId, Instant>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records a decided tool call of `project`; the input is kept as
/// [`input::input`] says, without the values of the project's secrets.
///
/// The decision is queued at once and recorded by a task that owns it, so
/// a project's entries keep the order its decisions were made in and
/// dropping the caller loses none. Projects do not wait for each other.
/// The receiver fires once it is recorded (or could not be).
pub(crate) fn tool_decided(
    core: &Core,
    project: &ProjectId,
    session: Option<&SessionId>,
    tool: &str,
    input: &serde_json::Value,
    allowed: bool,
    by: DecidedBy,
) -> oneshot::Receiver<()> {
    let (done, recorded) = oneshot::channel();
    let mut queue = lock(&core.audit_state().queue);
    let start = !queue.contains_key(project);
    let lane = queue.entry(project.clone()).or_default();
    lane.pending.push_back(Decided {
        session: session.cloned(),
        tool: tool.to_owned(),
        input: input.clone(),
        allowed,
        by,
        done,
    });
    if start {
        let more = lane.more.clone();
        tokio::spawn(drain(core.clone(), project.clone(), more));
    } else {
        lane.more.notify_waiters();
    }
    recorded
}

/// Hands the lane to a new task if its drain panics, so the decisions
/// behind the one that failed are still recorded.
struct Draining {
    core: Core,
    project: ProjectId,
    more: Arc<Notify>,
    done: bool,
}

impl Drop for Draining {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let mut queue = lock(&self.core.audit_state().queue);
        let waiting = queue
            .get(&self.project)
            .is_some_and(|lane| !lane.pending.is_empty());
        // Without a runtime (it is shutting down) nothing can record them;
        // dropping the lane drops their senders, which ends every wait.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) if waiting => {
                let (core, project) = (self.core.clone(), self.project.clone());
                runtime.spawn(drain(core, project, self.more.clone()));
            }
            _ => {
                queue.remove(&self.project);
            }
        }
    }
}

async fn drain(core: Core, project: ProjectId, more: Arc<Notify>) {
    let mut guard = Draining {
        core: core.clone(),
        project: project.clone(),
        more: more.clone(),
        done: false,
    };
    loop {
        let next = {
            let mut queue = lock(&core.audit_state().queue);
            match queue.get_mut(&project).and_then(|l| l.pending.pop_front()) {
                Some(next) => next,
                None => {
                    queue.remove(&project);
                    guard.done = true;
                    return;
                }
            }
        };
        let known = known_secrets(&core, &project, &more).await;
        let event = AuditEvent::ToolDecided {
            tool: input::cut_name(&next.tool, known.as_deref()),
            input: input::input(&next.tool, &next.input, known.as_deref()),
            allowed: next.allowed,
            by: next.by,
        };
        record(&core, &project, next.session.as_ref(), event);
        let _ = next.done.send(());
    }
}

/// The values of the project's secrets, or `None` if they cannot be read
/// now: from the cache, or from a read on a blocking thread as every
/// keychain call. A decision waits up to [`KEYCHAIN_TIMEOUT`] for a read it
/// starts, and no longer once another of the project's decisions waits
/// behind it; one that finds a read in flight, or a failed one within
/// [`RETRY_AFTER`], does not wait.
async fn known_secrets(
    core: &Core,
    project: &ProjectId,
    more: &Notify,
) -> Option<Arc<[SecretValue]>> {
    let read = match start_read(core, project) {
        Ok(known) => return known,
        Err(read) => read,
    };
    let mut read = pin!(tokio::time::timeout(KEYCHAIN_TIMEOUT, read));
    let mut behind = pin!(more.notified());
    behind.as_mut().enable();
    let waiting = lock(&core.audit_state().queue)
        .get(project)
        .is_some_and(|lane| !lane.pending.is_empty());
    if waiting {
        return None;
    }
    std::future::poll_fn(|cx| {
        if let Poll::Ready(result) = read.as_mut().poll(cx) {
            return Poll::Ready(result.ok().and_then(Result::ok).flatten());
        }
        behind.as_mut().poll(cx).map(|()| None)
    })
    .await
}

type Read = oneshot::Receiver<Option<Arc<[SecretValue]>>>;

/// What is known of the project's secrets now, or else a read just begun.
fn start_read(core: &Core, project: &ProjectId) -> Result<Option<Arc<[SecretValue]>>, Read> {
    let known = core.audit_state().known.clone();
    let generation = {
        let mut state = lock(&known);
        if let Some(values) = state.values.get(project) {
            return Ok(Some(values.clone()));
        }
        let failed = state.failed.get(project);
        if state.reading.contains(project) || failed.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return Ok(None);
        }
        state.reading.insert(project.clone());
        state.generation
    };
    let (done, read) = oneshot::channel();
    let secrets = core.secrets_store();
    let project = project.clone();
    tokio::spawn(async move {
        let reading = project.clone();
        let values = crate::secrets::blocking(move || {
            secrets
                .list(&reading)?
                .iter()
                .map(|secret| secrets.value(&reading, &secret.name))
                .collect::<Result<Arc<[SecretValue]>, _>>()
        })
        .await
        .ok();
        let mut state = lock(&known);
        state.reading.remove(&project);
        match &values {
            Some(values) if state.generation == generation => {
                state.failed.remove(&project);
                state.values.insert(project, values.clone());
            }
            Some(_) => {}
            None => {
                state.failed.insert(project, Instant::now());
            }
        }
        drop(state);
        let _ = done.send(values);
    });
    Err(read)
}

/// Drops what the core knows of the project's secret values; the secrets
/// broker calls it after every change to them.
pub(crate) fn forget_secrets(core: &Core, project: &ProjectId) {
    let mut known = lock(&core.audit_state().known);
    known.generation += 1;
    known.values.remove(project);
    known.failed.remove(project);
}

pub(crate) fn handle(core: &Core, request: AuditRequest) -> Result<Response, ProtocolError> {
    match request {
        AuditRequest::List {
            project,
            after,
            before,
            order,
            limit,
        } => {
            let limit = limit.unwrap_or(MAX_AUDIT_PAGE).min(MAX_AUDIT_PAGE);
            let range = AuditRange {
                after,
                before,
                order,
            };
            let entries = core.audit_log().list(&project, range, limit)?;
            Ok(Response::Audit(AuditResponse::Entries(entries)))
        }
    }
}

#[cfg(test)]
mod tests;
