//! `policy.*`: a project's rules that decide agents' tool calls before the
//! person is asked (`docs/specs/security.md`). Only the person changes them.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// What a rule does with a call it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    /// Allowed without asking, but only in a sandboxed session whose
    /// network is limited; elsewhere it asks.
    Auto,
    Ask,
    /// Refused, with the rule as the reason.
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRule {
    /// A tool name, such as `Bash` or `Edit`.
    pub tool: String,
    /// Tested against the call's main argument: a command prefix for
    /// `Bash`, a glob on the path relative to the session's folder for the
    /// file tools. `None` matches every call of the tool.
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    pub action: PolicyAction,
}

/// A project's rules. The empty policy asks about everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub rules: Vec<PolicyRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum PolicyRequest {
    /// Answers `policy.policy`.
    #[serde(rename = "policy.get")]
    Get { project: ProjectId },
    /// Replaces the project's policy. `bad_request` for an empty tool name,
    /// an empty pattern or a path glob that cannot match inside the
    /// project. Answers `policy.policy` with what was kept.
    #[serde(rename = "policy.set")]
    Set { project: ProjectId, policy: Policy },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PolicyResponse {
    #[serde(rename = "policy.policy")]
    Policy(Policy),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PolicyEvent {
    /// A project's policy was replaced.
    #[serde(rename = "policy.changed")]
    Changed { project: ProjectId },
}
