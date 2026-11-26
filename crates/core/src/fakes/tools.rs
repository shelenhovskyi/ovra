//! Tool calls as the Claude runtime (`ovra-agent-runtime`, `tools.rs`)
//! maps them, for fake agents and tests that use Claude's tool names.

use serde_json::Value;

use crate::protocol::agents::{ToolCall, ToolKind};
use crate::services::ToolRequest;
#[cfg(test)]
use crate::services::{AgentHost, BeforeTool};

/// The call `name` with `input`, with its kind and fields as the Claude
/// runtime reports them.
pub fn tool(name: &str, input: Value) -> ToolRequest {
    let text = |field: &str| input.get(field).and_then(Value::as_str).map(str::to_owned);
    let path = |field: &str| text(field).into_iter().collect();
    let mut call = ToolCall::default();
    let mut written = None;
    match name {
        "Bash" => {
            call.kind = ToolKind::Execute;
            call.command = text("command");
        }
        "Read" => (call.kind, call.paths) = (ToolKind::Read, path("file_path")),
        "NotebookRead" => (call.kind, call.paths) = (ToolKind::Read, path("notebook_path")),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let (field, content) = match name {
                "Edit" => ("file_path", "new_string"),
                "Write" => ("file_path", "content"),
                "MultiEdit" => ("file_path", "edits"),
                _ => ("notebook_path", "new_source"),
            };
            (call.kind, call.paths) = (ToolKind::Edit, path(field));
            written = input.get(content).map(|v| match v {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            });
        }
        "Glob" | "Grep" | "LS" | "WebSearch" => call.kind = ToolKind::Search,
        "WebFetch" => (call.kind, call.url) = (ToolKind::Fetch, text("url")),
        "Task" | "Agent" => call.kind = ToolKind::Subagent,
        "TodoWrite" => call.kind = ToolKind::Think,
        _ => {}
    }
    ToolRequest {
        name: name.to_owned(),
        input,
        call,
        written,
    }
}

/// What [`AgentHost::before_tool`] says about the call a Claude
/// `PreToolUse` hook input names (`tool_use_id`, `tool_name`,
/// `tool_input`); `Proceed` without a tool name, as the Claude runtime.
#[cfg(test)]
pub(crate) async fn before_hook(host: &AgentHost, hook: &Value) -> BeforeTool {
    let Some(name) = hook.get("tool_name").and_then(Value::as_str) else {
        return BeforeTool::Proceed;
    };
    let input = hook.get("tool_input").cloned().unwrap_or(Value::Null);
    let id = hook.get("tool_use_id").and_then(Value::as_str);
    host.before_tool(id, &tool(name, input)).await
}
