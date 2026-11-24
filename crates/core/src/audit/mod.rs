//! The audit log (`docs/specs/security.md`): the core writes one entry for
//! each event that matters for security, and `audit.list` reads them back.
//! The log itself is an [`AuditLog`](crate::services::AuditLog) service.

use crate::Core;
use crate::protocol::audit::{AuditRequest, AuditResponse, MAX_AUDIT_PAGE};
use crate::protocol::{ProtocolError, Response};

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
