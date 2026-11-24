//! The audit log's schema (`docs/specs/security.md`). Entries are only ever
//! appended; `audit.list` and the viewer come with task 4.E.

use serde::{Deserialize, Serialize};

use super::agents::{SandboxReport, SessionId, SessionMode};
use super::policy::Policy;
use super::projects::ProjectId;
use super::review::ReviewId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Increasing within one Ovra data folder.
    pub seq: u64,
    /// Unix time in milliseconds.
    pub at_ms: u64,
    pub project: ProjectId,
    pub session: Option<SessionId>,
    pub event: AuditEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditEvent {
    SessionStarted {
        mode: SessionMode,
        trusted: bool,
        /// `None`: the session ran without a sandbox.
        sandbox: Option<SandboxReport>,
    },
    ToolDecided {
        tool: String,
        input: AuditInput,
        allowed: bool,
        by: DecidedBy,
    },
    TrustChanged {
        trusted: bool,
        fingerprint: Option<String>,
    },
    PolicyChanged {
        policy: Policy,
    },
    ReviewDecided {
        review: ReviewId,
        action: ReviewAction,
    },
    NetworkRefused {
        host: String,
        reason: String,
    },
    /// A secret's name and where it went, never its value.
    SecretUsed {
        name: String,
        host: String,
    },
}

/// A tool call's input as the log keeps it: never file contents, and with
/// known secrets and token-like strings removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditInput {
    /// A file tool: the path and the SHA-256 of what it wrote.
    File {
        path: String,
        sha256: Option<String>,
    },
    /// Any other tool: its input as JSON, redacted and cut to 4 KiB.
    Json { text: String },
}

/// Who let a tool call through or refused it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecidedBy {
    Person,
    /// A project policy rule, as text.
    Policy {
        rule: String,
    },
    /// Ovra's role rules for its own IDE tools.
    Role,
    /// The session ended or stopped while the call waited.
    SessionEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Approved,
    PartlyApproved,
    Rejected,
    UndoneReject,
    Taken,
    UndoneTake,
}
