//! `permissions.*`: an agent asks to use a tool and a person answers.

use serde::{Deserialize, Serialize};

use super::agents::SessionId;
use super::policy::PolicyRule;
use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PermissionId(pub String);

/// A tool call waiting for a person's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: PermissionId,
    pub session: SessionId,
    /// The session's project, so a client that does not know the session
    /// still knows where the request comes from.
    pub project: ProjectId,
    /// The folder the session works in (the project root, or a worker's
    /// worktree); paths are judged against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Tool name as the agent reports it, e.g. `Bash` or `Edit`.
    pub tool: String,
    /// The tool's input, as the agent sent it.
    pub input: serde_json::Value,
    /// A one-line description from the agent, if it gave one.
    pub title: Option<String>,
    /// Why the agent's own rules ask a person, if it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PermissionReason>,
    /// The rule "Always allow" would add: the narrowest `auto` rule for this
    /// call. Offered only in a sandboxed session with a limited network,
    /// inside its folder, never for protected paths or calls a project
    /// rule asks about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always: Option<PolicyRule>,
}

/// Why an agent asks rather than deciding itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReason {
    /// For people, e.g. "This command requires approval".
    pub text: String,
    /// The agent's own category, e.g. `rule`, `hook` or `other`, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

/// The answer passed back to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAnswer {
    pub decision: Decision,
    /// Shown to the agent on `deny`, e.g. what to do instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl PermissionAnswer {
    pub fn allow() -> Self {
        Self {
            decision: Decision::Allow,
            message: None,
        }
    }

    pub fn deny(message: impl Into<String>) -> Self {
        Self {
            decision: Decision::Deny,
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PermissionsRequest {
    /// Requests still waiting for an answer, oldest first (after a reconnect
    /// or `info.resync`).
    #[serde(rename = "permissions.pending")]
    Pending,
    /// Answers a request. Answers `ok`; `not_found` if it was already answered
    /// or its session ended. With `always` (and `allow`), the request's
    /// `always` rule is added to the project's policy first; `bad_request`
    /// when the request offers none or a project rule now asks about it.
    #[serde(rename = "permissions.answer")]
    Answer {
        id: PermissionId,
        #[serde(flatten)]
        answer: PermissionAnswer,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        always: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PermissionsResponse {
    #[serde(rename = "permissions.pending")]
    Pending(Vec<PermissionRequest>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PermissionsEvent {
    #[serde(rename = "permissions.requested")]
    Requested(Box<PermissionRequest>),
    /// The request was answered, or dropped because its session ended
    /// (`decision: deny`).
    #[serde(rename = "permissions.resolved")]
    Resolved {
        id: PermissionId,
        decision: Decision,
    },
}
