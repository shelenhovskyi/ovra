//! Answers to an agent's calls of Ovra's own tools (`mcp__ovra__*`). They
//! are never put to a person: the caller's role and its task's limits
//! decide (`docs/specs/orchestration.md`). The tools check the same again
//! when they run.

use super::tasks;
use crate::Core;
use crate::protocol::agents::SessionId;
use crate::protocol::permissions::PermissionAnswer;
use crate::services::ToolRole;

/// How Claude Code names the tools of the in-process `ovra` MCP server.
pub(crate) const IDE_TOOL_PREFIX: &str = "mcp__ovra__";

/// Tools only a coordinator may call.
const COORDINATOR_TOOLS: [&str; 5] = [
    "spawn_agent",
    "list_agents",
    "read_output",
    "send_message",
    "stop_agent",
];

/// Tools every session with IDE tools may call.
const SHARED_TOOLS: [&str; 2] = ["request_review", "open_in_ui"];

/// Decides a call of the IDE tool `name` (without the prefix) by `session`.
pub(crate) fn decide(
    core: &Core,
    session: &SessionId,
    name: &str,
    input: &serde_json::Value,
) -> PermissionAnswer {
    let role = core
        .sessions()
        .find(session, |s| s.launch().and_then(|l| l.tools))
        .ok()
        .flatten();
    match decide_for(core, session, role, name, input) {
        Ok(()) => PermissionAnswer::allow(),
        Err(reason) => PermissionAnswer::deny(reason),
    }
}

fn decide_for(
    core: &Core,
    session: &SessionId,
    role: Option<ToolRole>,
    name: &str,
    input: &serde_json::Value,
) -> Result<(), String> {
    let Some(role) = role else {
        return Err("this session has no Ovra tools".to_owned());
    };
    let coordinator_only = COORDINATOR_TOOLS.contains(&name);
    if !coordinator_only && !SHARED_TOOLS.contains(&name) {
        return Err(format!("Ovra has no tool named {name}"));
    }
    match role {
        ToolRole::Worker if coordinator_only => {
            Err(format!("only the task's coordinator may use {name}"))
        }
        ToolRole::Worker if name == "request_review" && input.get("agent_id").is_some() => {
            Err("a worker may ask for a review of its own work only".to_owned())
        }
        ToolRole::Worker => Ok(()),
        ToolRole::Coordinator => {
            let task = tasks::of_coordinator(core, session).map_err(|e| e.message)?;
            match tasks::spawn_blocked(core, &task) {
                Some(reason) if name == "spawn_agent" => Err(reason),
                _ => Ok(()),
            }
        }
    }
}
