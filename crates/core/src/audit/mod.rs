//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

mod input;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::oneshot;

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

/// How long a keychain read may take before an entry is kept as if the
/// secrets could not be read. A late answer still fills the cache.
pub(crate) const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// The core's audit state: tool decisions waiting to be recorded, in the
/// order they were made, and the secret values read for redaction.
#[derive(Default)]
pub(crate) struct State {
    queue: Mutex<Queue>,
    known: Arc<Mutex<Known>>,
}

/// Never shows a decision's input or a secret's value.
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("audit::State").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Decided>,
    draining: bool,
}

struct Decided {
    project: ProjectId,
    session: Option<SessionId>,
    tool: String,
    input: serde_json::Value,
    allowed: bool,
    by: DecidedBy,
    done: oneshot::Sender<()>,
}

/// Each project's secret values, as last read. `generation` changes with
/// every change to any project's secrets, so a read that began before one
/// is not kept.
#[derive(Default)]
struct Known {
    values: HashMap<ProjectId, Arc<[SecretValue]>>,
    generation: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records a decided tool call of `project`; the input is kept as
/// [`input::input`] says, without the values of the project's secrets.
///
/// The decision is queued at once and recorded by a task that owns it, so
/// entries keep the order decisions were made in and dropping the caller
/// loses none. The receiver fires once it is recorded (or could not be).
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
    queue.pending.push_back(Decided {
        project: project.clone(),
        session: session.cloned(),
        tool: tool.to_owned(),
        input: input.clone(),
        allowed,
        by,
        done,
    });
    if !queue.draining {
        queue.draining = true;
        tokio::spawn(drain(core.clone()));
    }
    recorded
}

async fn drain(core: Core) {
    loop {
        let next = {
            let mut queue = lock(&core.audit_state().queue);
            match queue.pending.pop_front() {
                Some(next) => next,
                None => {
                    queue.draining = false;
                    return;
                }
            }
        };
        let known = known_secrets(&core, &next.project).await;
        let event = AuditEvent::ToolDecided {
            tool: input::cut_name(&next.tool, known.as_deref()),
            input: input::input(&next.tool, &next.input, known.as_deref()),
            allowed: next.allowed,
            by: next.by,
        };
        record(&core, &next.project, next.session.as_ref(), event);
        let _ = next.done.send(());
    }
}

/// The values of the project's secrets: from the cache, or read on a
/// blocking thread as every keychain call. `None` if any of them cannot
/// be read within [`KEYCHAIN_TIMEOUT`].
async fn known_secrets(core: &Core, project: &ProjectId) -> Option<Arc<[SecretValue]>> {
    let known = core.audit_state().known.clone();
    let generation = {
        let known = lock(&known);
        if let Some(values) = known.values.get(project) {
            return Some(values.clone());
        }
        known.generation
    };
    let secrets = core.secrets_store();
    let project = project.clone();
    let read = crate::secrets::blocking(move || {
        let values: Arc<[SecretValue]> = secrets
            .list(&project)?
            .iter()
            .map(|secret| secrets.value(&project, &secret.name))
            .collect::<Result<_, _>>()?;
        let mut known = lock(&known);
        if known.generation == generation {
            known.values.insert(project, values.clone());
        }
        Ok(values)
    });
    tokio::time::timeout(KEYCHAIN_TIMEOUT, read)
        .await
        .ok()?
        .ok()
}

/// Drops what the core knows of the project's secret values; the secrets
/// broker calls it after every change to them.
pub(crate) fn forget_secrets(core: &Core, project: &ProjectId) {
    let mut known = lock(&core.audit_state().known);
    known.generation += 1;
    known.values.remove(project);
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
