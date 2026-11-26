//! `audit.*`: the audit log's schema (`docs/specs/security.md`). Entries
//! are only ever appended; no method edits or deletes one.

use serde::{Deserialize, Serialize};

use super::agents::{SandboxReport, SessionId, SessionMode};
use super::board::{Author, EntryId};
use super::orchestration::TaskId;
use super::policy::{Policy, PolicyRule};
use super::projects::ProjectId;
use super::review::ReviewId;
use super::servers::{ServerId, StartedBy};

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
    /// `by` added `rule` to the policy from an approval ("Always allow");
    /// the entry's session is the one that asked.
    RuleAdded {
        rule: PolicyRule,
        by: DecidedBy,
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
    ServerStarted {
        server: ServerId,
        name: String,
        generation: u64,
        by: StartedBy,
    },
    /// `code` is missing when it was killed by a signal.
    ServerStopped {
        server: ServerId,
        name: String,
        generation: u64,
        code: Option<i32>,
    },
    /// The session's netproxy may reach the server on `localhost:<port>`
    /// while this generation runs (ADR 0008).
    LoopbackGranted {
        server: ServerId,
        port: u16,
        generation: u64,
    },
    LoopbackRevoked {
        server: ServerId,
        port: u16,
        generation: u64,
    },
    /// A language server for the project's files (ADR 0007); `sandbox`
    /// is `None` when it runs without one.
    LanguageServerStarted {
        server: crate::protocol::lsp::ServerId,
        name: String,
        language: String,
        sandbox: Option<String>,
    },
    /// `reason` says why it failed; `None` when it was stopped or exited
    /// cleanly.
    LanguageServerStopped {
        server: crate::protocol::lsp::ServerId,
        name: String,
        language: String,
        reason: Option<String>,
    },
    /// A worker's changes went out as `commit` to `branch` on `remote`:
    /// the person's `pr.create` or `pr.update`, or the worker's `push`
    /// after the `Push` decision recorded before it.
    BranchPushed {
        review: ReviewId,
        remote: String,
        branch: String,
        commit: String,
    },
    /// The person opened pull request `number` of a worker's item.
    PullRequestOpened {
        review: ReviewId,
        remote: String,
        number: u64,
        url: String,
    },
    /// An entry on a task's board (`docs/specs/task-board.md`). `text`
    /// is what it says (a claim's paths, a question, a note) without
    /// header values or token-like words and cut to 4 KiB; the entry's
    /// session is the agent that wrote it.
    BoardEntry {
        task: TaskId,
        entry: EntryId,
        author: Author,
        what: BoardEntryKind,
        text: String,
    },
    /// Whether a session's browser may load `origin`, which is neither a
    /// granted server nor decided before for the session (ADR 0009).
    BrowserNavigation {
        origin: String,
        allowed: bool,
        by: NavigationBy,
    },
}

/// What kind of entry a board entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardEntryKind {
    Claim,
    Release,
    Question,
    Answer,
    Note,
    Ovra,
}

/// Who decided a browser navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationBy {
    Person,
    /// Nobody answered in time: refused.
    NoAnswer,
    /// The session ended while the navigation waited: refused.
    SessionEnded,
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
    /// A person on a paired phone (ADR 0014), by the phone's name.
    Phone {
        device: String,
    },
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

/// The most entries one `audit.list` returns.
pub const MAX_AUDIT_PAGE: usize = 500;

/// Which of a project's entries one `audit.list` reads: those with a
/// `seq` above `after` and below `before`, in `order`. A page holds the
/// first entries in that order, so `newest_first` without `before` opens
/// the end of a long log, and `before` the lowest `seq` shown pages back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditRange {
    pub after: Option<u64>,
    pub before: Option<u64>,
    pub order: AuditOrder,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOrder {
    #[default]
    OldestFirst,
    NewestFirst,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum AuditRequest {
    /// Up to `limit` (at most [`MAX_AUDIT_PAGE`]) of the project's entries
    /// as [`AuditRange`] says. Answers `audit.entries`.
    #[serde(rename = "audit.list")]
    List {
        project: ProjectId,
        #[serde(default)]
        after: Option<u64>,
        #[serde(default)]
        before: Option<u64>,
        #[serde(default)]
        order: AuditOrder,
        #[serde(default)]
        limit: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AuditResponse {
    #[serde(rename = "audit.entries")]
    Entries(Vec<AuditEntry>),
}

/// Pushed events of the `audit` domain (named so, because [`AuditEvent`]
/// is what an entry records).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AuditLogEvent {
    /// An entry was added, for live viewers.
    #[serde(rename = "audit.appended")]
    Appended(AuditEntry),
}
