//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

mod input;

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

/// The entry for a decided tool call of `project`; the input is kept as
/// [`input::input`] says, without the values of the project's secrets.
pub(crate) async fn tool_decided(
    core: &Core,
    project: &ProjectId,
    tool: &str,
    input: &serde_json::Value,
    allowed: bool,
    by: DecidedBy,
) -> AuditEvent {
    let known = known_secrets(core, project).await;
    AuditEvent::ToolDecided {
        tool: input::cut_name(tool, known.as_deref()),
        input: input::input(tool, input, known.as_deref()),
        allowed,
        by,
    }
}

/// The values of the project's secrets, read on a blocking thread as every
/// keychain call; `None` if any of them cannot be read.
async fn known_secrets(core: &Core, project: &ProjectId) -> Option<Vec<SecretValue>> {
    let secrets = core.secrets_store();
    let project = project.clone();
    crate::secrets::blocking(move || {
        secrets
            .list(&project)?
            .iter()
            .map(|secret| secrets.value(&project, &secret.name))
            .collect()
    })
    .await
    .ok()
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
