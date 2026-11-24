//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

mod input;

use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::audit::{
    AuditEvent, AuditLogEvent, AuditRequest, AuditResponse, DecidedBy, MAX_AUDIT_PAGE,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ProtocolError, Response};

/// Appends `event` and shows it to live viewers. A log that cannot be
/// written does not stop the work it records, as with the session store.
pub(crate) fn record(
    core: &Core,
    project: &ProjectId,
    session: Option<&SessionId>,
    event: AuditEvent,
) {
    if let Ok(entry) = core.audit_log().append(project, session, event) {
        core.emit(AuditLogEvent::Appended(entry));
    }
}

/// The entry for a decided tool call; the input is kept as
/// [`input::input`] says.
pub(crate) fn tool_decided(
    tool: &str,
    input: &serde_json::Value,
    allowed: bool,
    by: DecidedBy,
) -> AuditEvent {
    AuditEvent::ToolDecided {
        tool: input::cut_name(tool),
        input: input::input(tool, input),
        allowed,
        by,
    }
}

pub(crate) fn handle(core: &Core, request: AuditRequest) -> Result<Response, ProtocolError> {
    match request {
        AuditRequest::List {
            project,
            after,
            limit,
        } => {
            let limit = limit.unwrap_or(MAX_AUDIT_PAGE).min(MAX_AUDIT_PAGE);
            let entries = core.audit_log().list(&project, after, limit)?;
            Ok(Response::Audit(AuditResponse::Entries(entries)))
        }
    }
}

#[cfg(test)]
mod tests;
