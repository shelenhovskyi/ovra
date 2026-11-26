//! `setup.*`: whether what Ovra's features need is on this computer, for
//! the "Check setup" pane. Only facts: the UI words each problem and its fix.

use serde::{Deserialize, Serialize};

use super::agents::{AgentKind, AgentSetup};
use super::pr::GhStatus;
use super::projects::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SetupRequest {
    /// Checks every agent provider, git, the sandbox and, with a project,
    /// `gh` for the host of the project's `origin`. Answers `setup.report`.
    #[serde(rename = "setup.check")]
    Check {
        #[serde(default)]
        project: Option<ProjectId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SetupResponse {
    #[serde(rename = "setup.report")]
    Report(SetupReport),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupReport {
    /// Every provider the runtime can start, in its order (Claude first).
    pub agents: Vec<AgentCheck>,
    pub git: Probe<GitTool>,
    /// `skipped` without a project: sign-in is per host, and the host is
    /// the one the project's `origin` names.
    pub gh: Probe<GhStatus>,
    pub sandbox: SandboxCheck,
}

/// One provider's `agents.setup`, with what the person chose for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCheck {
    pub id: AgentKind,
    /// What the UI calls it, such as `Claude`.
    pub name: String,
    pub setup: AgentSetup,
}

/// The git command line, as git panels and worktrees run it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTool {
    pub installed: bool,
    pub version: Option<String>,
}

/// One check's outcome: what it found, why it could not be made, or that
/// it was not made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Probe<T> {
    Checked { found: T },
    Failed { message: String },
    Skipped,
}

/// Whether agents' processes can run in this OS's sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SandboxCheck {
    Available {
        /// Such as `bubblewrap`.
        name: String,
        /// What it does not cover, each a short phrase.
        gaps: Vec<String>,
    },
    Unavailable {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::agents::SetupState;
    use crate::protocol::{Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let report = SetupReport {
            agents: vec![AgentCheck {
                id: AgentKind::Claude,
                name: "Claude".into(),
                setup: AgentSetup {
                    path: None,
                    executable: Some("/usr/local/bin/claude".into()),
                    version: Some("2.1.331".into()),
                    state: SetupState::SignedOut,
                    billing: None,
                    api_key_in_env: false,
                },
            }],
            git: Probe::Checked {
                found: GitTool {
                    installed: true,
                    version: Some("2.47.1".into()),
                },
            },
            gh: Probe::Failed {
                message: "gh --version timed out".into(),
            },
            sandbox: SandboxCheck::Unavailable {
                reason: "bubblewrap is not installed".into(),
            },
        };
        let skipped = SetupReport {
            agents: Vec::new(),
            git: Probe::Checked {
                found: GitTool::default(),
            },
            gh: Probe::Skipped,
            sandbox: SandboxCheck::Available {
                name: "bubblewrap".into(),
                gaps: vec!["DNS lookups".into()],
            },
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Setup(SetupRequest::Check { project: None }),
                Request::Setup(SetupRequest::Check { project: Some(ProjectId("p1".into())) }),
            ],
            "responses": [
                Response::Setup(SetupResponse::Report(report)),
                Response::Setup(SetupResponse::Report(skipped)),
            ],
        }));
    }
}
