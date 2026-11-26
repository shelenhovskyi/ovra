//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

mod input;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::{
    AuditEvent, AuditLogEvent, AuditRange, AuditRequest, AuditResponse, DecidedBy, MAX_AUDIT_PAGE,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::secrets::SecretValue;
use crate::protocol::{ProtocolError, Response};
use crate::services::ToolRequest;

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
        AuditEvent::RuleAdded { .. } => "rule_added",
        AuditEvent::ReviewDecided { .. } => "review_decided",
        AuditEvent::NetworkRefused { .. } => "network_refused",
        AuditEvent::SecretUsed { .. } => "secret_used",
        AuditEvent::ServerStarted { .. } => "server_started",
        AuditEvent::ServerStopped { .. } => "server_stopped",
        AuditEvent::LoopbackGranted { .. } => "loopback_granted",
        AuditEvent::LoopbackRevoked { .. } => "loopback_revoked",
        AuditEvent::LanguageServerStarted { .. } => "language_server_started",
        AuditEvent::LanguageServerStopped { .. } => "language_server_stopped",
        AuditEvent::BrowserNavigation { .. } => "browser_navigation",
        AuditEvent::BranchPushed { .. } => "branch_pushed",
        AuditEvent::PullRequestOpened { .. } => "pull_request_opened",
        AuditEvent::BoardEntry { .. } => "board_entry",
    }
}

/// How long after a keychain read began its project's decisions are kept
/// as if the secrets could not be read. A late answer still fills the cache.
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
/// task drains it.
type Lane = VecDeque<Decided>;

struct Decided {
    session: Option<SessionId>,
    tool: ToolRequest,
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
    reading: HashMap<ProjectId, Reading>,
    failed: HashMap<ProjectId, Instant>,
}

/// What a read gives: the values, or `None` if they could not be read.
type Outcome = Option<Arc<[SecretValue]>>;

/// A read in flight. Every decision of the project waits on it until
/// `deadline`, set when it began, so a hung keychain costs one timeout per
/// read, not one per decision.
struct Reading {
    generation: u64,
    deadline: Instant,
    outcome: watch::Receiver<Option<Outcome>>,
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
    tool: &ToolRequest,
    allowed: bool,
    by: DecidedBy,
) -> oneshot::Receiver<()> {
    let (done, recorded) = oneshot::channel();
    let mut queue = lock(&core.audit_state().queue);
    let start = !queue.contains_key(project);
    queue
        .entry(project.clone())
        .or_default()
        .push_back(Decided {
            session: session.cloned(),
            tool: tool.clone(),
            allowed,
            by,
            done,
        });
    if start {
        tokio::spawn(drain(core.clone(), project.clone()));
    }
    recorded
}

/// Ends a lane's drain that stopped early. One that was running (its
/// recording panicked, or the runtime is shutting down) hands the lane to a
/// new task; one that never ran, or with no runtime left, drops the lane,
/// which drops the senders of its decisions and so ends every wait.
struct Draining {
    core: Core,
    project: ProjectId,
    started: bool,
    done: bool,
}

impl Draining {
    /// The lane is empty and gone: nothing is left to hand on.
    fn finish(&mut self) {
        self.done = true;
    }
}

impl Drop for Draining {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // Never the entry: it may hold what the agent wrote.
        tracing::warn!(
            project = %self.project.0,
            "recording a tool decision in the audit log stopped unexpectedly"
        );
        let mut queue = lock(&self.core.audit_state().queue);
        let waiting = queue
            .get(&self.project)
            .is_some_and(|lane| !lane.is_empty());
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) if waiting && self.started => {
                // Unlocked first: a runtime shutting down drops the new
                // task at once, and its guard takes the lock.
                drop(queue);
                runtime.spawn(drain(self.core.clone(), self.project.clone()));
            }
            _ => {
                queue.remove(&self.project);
            }
        }
    }
}

/// The guard is made here, outside the task's body, so a task dropped
/// before it first runs still ends its lane.
fn drain(core: Core, project: ProjectId) -> impl Future<Output = ()> + Send + 'static {
    let mut guard = Draining {
        core,
        project,
        started: false,
        done: false,
    };
    async move {
        guard.started = true;
        let (core, project) = (guard.core.clone(), guard.project.clone());
        loop {
            let next = {
                let mut queue = lock(&core.audit_state().queue);
                match queue.get_mut(&project).and_then(VecDeque::pop_front) {
                    Some(next) => next,
                    None => {
                        queue.remove(&project);
                        guard.finish();
                        return;
                    }
                }
            };
            let known = known_secrets(&core, &project).await;
            let event = AuditEvent::ToolDecided {
                tool: input::cut_name(&next.tool.name, known.as_deref()),
                input: input::input(&next.tool, known.as_deref()),
                allowed: next.allowed,
                by: next.by,
            };
            record(&core, &project, next.session.as_ref(), event);
            let _ = next.done.send(());
        }
    }
}

pub(crate) use input::holds_secret;

/// Records `event(text)` for `text` an agent wrote, once the project's
/// secret values are known: without them, as tool inputs are kept, and
/// `[redacted]` when they cannot be read.
pub(crate) async fn agent_text(
    core: Core,
    project: ProjectId,
    session: Option<SessionId>,
    text: String,
    event: impl FnOnce(String) -> AuditEvent,
) {
    let known = known_secrets(&core, &project).await;
    let text = input::redacted_text(&text, known.as_deref());
    record(&core, &project, session.as_ref(), event(text));
}

/// The values of the project's secrets for checking text the person adds
/// to a message; `None` if they cannot be read now.
pub(crate) async fn secret_values(core: &Core, project: &ProjectId) -> Outcome {
    known_secrets(core, project).await
}

/// The values of the project's secrets, or `None` if they cannot be read
/// now: from the cache, or from the project's read in flight (started here
/// if there is none) on a blocking thread as every keychain call. A
/// decision waits for that read until its deadline, [`KEYCHAIN_TIMEOUT`]
/// after it began; within [`RETRY_AFTER`] of a failed read it does not wait.
async fn known_secrets(core: &Core, project: &ProjectId) -> Outcome {
    let (mut outcome, deadline) = match start_read(core, project) {
        Ok(known) => return known,
        Err(reading) => reading,
    };
    match tokio::time::timeout_at(deadline, outcome.wait_for(Option::is_some)).await {
        Ok(Ok(seen)) => (*seen).clone().flatten(),
        _ => None,
    }
}

/// What is known of the project's secrets now, or else the read in flight
/// and its deadline, begun here if there was none.
fn start_read(
    core: &Core,
    project: &ProjectId,
) -> Result<Outcome, (watch::Receiver<Option<Outcome>>, Instant)> {
    let known = core.audit_state().known.clone();
    let mut state = lock(&known);
    if let Some(values) = state.values.get(project) {
        return Ok(Some(values.clone()));
    }
    // A read whose task died (its runtime shut down) never answers: start
    // another in its place.
    match state.reading.get(project) {
        Some(reading) if reading.outcome.has_changed().is_ok() => {
            return Err((reading.outcome.clone(), reading.deadline));
        }
        Some(_) => {
            state.reading.remove(project);
        }
        None => {}
    }
    if state
        .failed
        .get(project)
        .is_some_and(|at| at.elapsed() < RETRY_AFTER)
    {
        return Ok(None);
    }
    let generation = state.generation;
    let deadline = Instant::now() + KEYCHAIN_TIMEOUT;
    let (done, outcome) = watch::channel(None);
    let reading = Reading {
        generation,
        deadline,
        outcome: outcome.clone(),
    };
    state.reading.insert(project.clone(), reading);
    drop(state);
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
        // `forget_secrets` may have dropped this read and begun another.
        if state
            .reading
            .get(&project)
            .is_some_and(|r| r.generation == generation)
        {
            state.reading.remove(&project);
        }
        if state.generation == generation {
            match &values {
                Some(values) => {
                    state.failed.remove(&project);
                    state.values.insert(project, values.clone());
                }
                None => {
                    state.failed.insert(project, Instant::now());
                }
            }
        }
        drop(state);
        done.send_replace(Some(values));
    });
    Err((outcome, deadline))
}

/// Drops what the core knows of the project's secret values, its read in
/// flight and its last failure, so the next decision reads them again; the
/// secrets broker calls it after every change to them.
pub(crate) fn forget_secrets(core: &Core, project: &ProjectId) {
    let mut known = lock(&core.audit_state().known);
    known.generation += 1;
    known.values.remove(project);
    known.reading.remove(project);
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
