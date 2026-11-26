//! Claude's tools in Ovra's shared terms (ADR 0011): each call's kind and
//! the fields shared code reads (paths, command, URL), next to its own name
//! and input.

use ovra_core::protocol::agents::{ToolCall, ToolKind};
use ovra_core::services::ToolRequest;
use serde_json::Value;

/// The call of Claude's tool `name` with `input`. Tools Ovra does not know
/// (MCP tools, `mcp__*`, and new built-in ones) are `other`.
pub(crate) fn tool(name: &str, input: Value) -> ToolRequest {
    let text = |field: &str| input.get(field).and_then(Value::as_str).map(str::to_owned);
    let path = |field: &str| text(field).into_iter().collect::<Vec<_>>();
    // What an edit writes, for the audit log's hash: the field's text, or
    // its JSON (`MultiEdit`'s list of edits).
    let content = |field: &str| {
        input.get(field).map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
    };
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
            let (field, writes) = match name {
                "Edit" => ("file_path", "new_string"),
                "Write" => ("file_path", "content"),
                "MultiEdit" => ("file_path", "edits"),
                _ => ("notebook_path", "new_source"),
            };
            (call.kind, call.paths) = (ToolKind::Edit, path(field));
            written = content(writes);
        }
        // `WebSearch` looks, as `Grep` does: it reads no page itself.
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::tool;

    #[test]
    fn claudes_tools_map_to_kinds_and_fields() {
        let cases = [
            (
                "Bash",
                json!({"command": "git status", "description": "Status"}),
            ),
            ("Read", json!({"file_path": "/w/src/a.rs"})),
            ("NotebookRead", json!({"notebook_path": "/w/n.ipynb"})),
            (
                "Edit",
                json!({"file_path": "/w/a.rs", "old_string": "x", "new_string": "y"}),
            ),
            (
                "Write",
                json!({"file_path": "b.rs", "content": "fn main() {}"}),
            ),
            (
                "MultiEdit",
                json!({"file_path": "/w/a.rs", "edits": [{"old_string": "x", "new_string": "y"}]}),
            ),
            (
                "NotebookEdit",
                json!({"notebook_path": "/w/n.ipynb", "new_source": "print(1)"}),
            ),
            ("Glob", json!({"pattern": "**/*.rs"})),
            ("Grep", json!({"pattern": "fn main", "path": "/w"})),
            ("LS", json!({"path": "/w"})),
            ("WebSearch", json!({"query": "serde"})),
            (
                "WebFetch",
                json!({"url": "https://docs.rs/", "prompt": "x"}),
            ),
            ("Task", json!({"description": "Look", "prompt": "look"})),
            ("Agent", json!({"prompt": "look"})),
            ("TodoWrite", json!({"todos": []})),
            ("BashOutput", json!({"bash_id": "1"})),
            ("mcp__ovra__spawn_agent", json!({"task": "x"})),
            ("mcp__github__create_issue", json!({"title": "x"})),
            // A field of the wrong type is left out, never guessed.
            ("Bash", json!({"command": ["rm", "-rf"]})),
            ("Edit", json!({"file_path": 1, "new_string": "y"})),
        ];
        let lines: Vec<String> = cases
            .into_iter()
            .map(|(name, input)| {
                let request = tool(name, input.clone());
                // The core's fake maps Claude's tools for core tests; both
                // must agree.
                assert_eq!(request, ovra_core::fakes::tool(name, input), "{name}");
                let call = serde_json::to_string(&request.call).unwrap_or_default();
                let written = request
                    .written
                    .map_or(String::new(), |w| format!(" writes {w}"));
                format!("{name} => {call}{written}")
            })
            .collect();
        insta::assert_snapshot!(lines.join("\n"));
    }
}
