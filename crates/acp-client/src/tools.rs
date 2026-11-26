//! An ACP tool call in Ovra's shared terms (ADR 0011). ACP already says
//! what a call does (`kind`) and where (`locations`); the command and URL
//! come from the raw input, the text an edit writes from its diffs.

use ovra_core::protocol::agents::{ToolCall, ToolKind};
use ovra_core::services::ToolRequest;
use serde_json::Value;

use crate::v1::{Content, ToolCallFields, ToolContent, ToolStatus};

/// What Ovra names the call: ACP has titles, no tool names.
pub(crate) fn name(call: &ToolCallFields) -> String {
    call.title.clone().unwrap_or_else(|| {
        let kind = serde_json::to_value(call.kind.unwrap_or_default()).unwrap_or_default();
        kind.as_str().unwrap_or("other").to_owned()
    })
}

pub(crate) fn request(call: &ToolCallFields) -> ToolRequest {
    let input = call.raw_input.clone().unwrap_or(Value::Null);
    let text = |field: &str| input.get(field).and_then(Value::as_str).map(str::to_owned);
    let kind = call.kind.unwrap_or_default();
    let shared = ToolCall {
        kind,
        paths: call
            .locations
            .iter()
            .flatten()
            .map(|l| l.path.clone())
            .collect(),
        // A field of another type is left out, never guessed.
        command: text("command").filter(|_| kind == ToolKind::Execute),
        url: text("url").filter(|_| kind == ToolKind::Fetch),
    };
    let diffs: Vec<&str> = call
        .content
        .iter()
        .flatten()
        .filter_map(|c| match c {
            ToolContent::Diff { new_text } => Some(new_text.as_str()),
            _ => None,
        })
        .collect();
    let written = (kind == ToolKind::Edit && !diffs.is_empty()).then(|| diffs.join("\n"));
    ToolRequest {
        name: name(call),
        input,
        call: shared,
        written,
    }
}

/// The result of a finished call: its text content, else its raw output.
/// `None` while it runs.
pub(crate) fn result(call: &ToolCallFields) -> Option<(String, bool)> {
    let failed = match call.status? {
        ToolStatus::Completed => false,
        ToolStatus::Failed => true,
        ToolStatus::Running => return None,
    };
    let texts: Vec<&str> = call
        .content
        .iter()
        .flatten()
        .filter_map(|c| match c {
            ToolContent::Content {
                content: Content::Text { text },
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let output = if texts.is_empty() {
        match &call.raw_output {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    } else {
        texts.join("\n")
    };
    Some((output, failed))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(fields: Value) -> ToolCallFields {
        serde_json::from_value(fields).expect("a tool call")
    }

    #[test]
    fn acp_calls_map_to_kinds_and_fields() {
        let cases = [
            json!({"toolCallId": "1", "title": "ls", "kind": "execute",
                "rawInput": {"command": "ls -la"}}),
            json!({"toolCallId": "2", "title": "Read a.rs", "kind": "read",
                "locations": [{"path": "/w/a.rs", "line": 1}, {"path": "b.rs"}]}),
            json!({"toolCallId": "3", "title": "Edit a.rs", "kind": "edit",
                "locations": [{"path": "/w/a.rs"}],
                "content": [{"type": "diff", "path": "/w/a.rs", "oldText": "x", "newText": "y"}]}),
            json!({"toolCallId": "4", "title": "Fetch", "kind": "fetch",
                "rawInput": {"url": "https://docs.rs/"}}),
            json!({"toolCallId": "5", "kind": "search", "rawInput": {"pattern": "fn"}}),
            // A command of another kind, or of another type, is no command.
            json!({"toolCallId": "6", "title": "x", "kind": "think", "rawInput": {"command": "ls"}}),
            json!({"toolCallId": "7", "title": "x", "kind": "execute",
                "rawInput": {"command": ["rm", "-rf"]}}),
            json!({"toolCallId": "8", "kind": "switch_mode"}),
            json!({"toolCallId": "9"}),
        ];
        let lines: Vec<String> = cases
            .into_iter()
            .map(|fields| {
                let request = request(&call(fields));
                let shared = serde_json::to_string(&request.call).unwrap_or_default();
                let written = request
                    .written
                    .map_or(String::new(), |w| format!(" writes {w}"));
                format!("{} {} => {shared}{written}", request.name, request.input)
            })
            .collect();
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn a_finished_call_has_a_result() {
        let running = call(json!({"toolCallId": "1", "status": "in_progress"}));
        assert_eq!(result(&running), None);
        let texts = call(
            json!({"toolCallId": "1", "status": "completed", "content": [
                {"type": "content", "content": {"type": "text", "text": "a"}},
                {"type": "terminal", "terminalId": "t"},
                {"type": "content", "content": {"type": "text", "text": "b"}},
            ]}),
        );
        assert_eq!(result(&texts), Some(("a\nb".into(), false)));
        let raw = call(json!({"toolCallId": "1", "status": "failed", "rawOutput": {"code": 1}}));
        assert_eq!(result(&raw), Some((r#"{"code":1}"#.into(), true)));
        let said = call(json!({"toolCallId": "1", "status": "failed", "rawOutput": "no"}));
        assert_eq!(result(&said), Some(("no".into(), true)));
    }
}
