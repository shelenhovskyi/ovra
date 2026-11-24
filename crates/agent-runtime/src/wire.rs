//! The stream-json lines `claude` reads and writes
//! (`docs/specs/claude-cli-protocol.md`). Parsing is lenient: unknown types
//! and fields are ignored, so newer CLI versions can add to the protocol.

use ovra_core::protocol::agents::{AgentMessage, TurnUsage};
use serde_json::{Value, json};

/// Request id of our `initialize` control request.
pub(crate) const INIT_ID: &str = "init";
/// Callback id of the `PreToolUse` hook registered at `initialize`.
pub(crate) const POLICY_HOOK: &str = "ovra_policy";

/// What one stdout line means to the driver.
#[derive(Debug, PartialEq)]
pub(crate) enum Incoming {
    /// `system/init`: the session is up and works on the turn.
    Started,
    /// Transcript entries from an `assistant` or `user` message.
    Messages(Vec<AgentMessage>),
    /// `result`: the turn ended, with an error when it did not succeed.
    /// The cost is the session's total so far.
    TurnEnd {
        usage: TurnUsage,
        error: Option<String>,
    },
    /// The CLI asks whether a tool call may run.
    CanUseTool {
        request_id: String,
        tool: String,
        input: Value,
        title: Option<String>,
    },
    /// The CLI runs one of our hooks.
    Hook {
        request_id: String,
        callback_id: String,
        input: Value,
    },
    /// A control request we do not handle; answered with an error.
    UnknownControl { request_id: String, subtype: String },
    /// The CLI answers one of our control requests.
    ControlResponse {
        request_id: String,
        error: Option<String>,
    },
    /// The CLI no longer needs an answer to one of its requests.
    Cancel { request_id: String },
    /// Anything else, including lines that are not JSON.
    Ignored,
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn parse(line: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Incoming::Ignored;
    };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "system" if v.get("subtype").and_then(Value::as_str) == Some("init") => Incoming::Started,
        "assistant" | "user" => Incoming::Messages(messages(&v)),
        "result" => turn_end(&v),
        "control_request" => control_request(&v),
        "control_response" => {
            let response = v.get("response").unwrap_or(&Value::Null);
            let Some(request_id) = str_field(response, "request_id") else {
                return Incoming::Ignored;
            };
            let error = match response.get("subtype").and_then(Value::as_str) {
                Some("success") => None,
                _ => Some(str_field(response, "error").unwrap_or_else(|| "failed".to_owned())),
            };
            Incoming::ControlResponse { request_id, error }
        }
        "control_cancel_request" => match str_field(&v, "request_id") {
            Some(request_id) => Incoming::Cancel { request_id },
            None => Incoming::Ignored,
        },
        _ => Incoming::Ignored,
    }
}

fn control_request(v: &Value) -> Incoming {
    let (Some(request_id), Some(request)) = (str_field(v, "request_id"), v.get("request")) else {
        return Incoming::Ignored;
    };
    let subtype = str_field(request, "subtype").unwrap_or_default();
    match subtype.as_str() {
        "can_use_tool" => match str_field(request, "tool_name") {
            Some(tool) => Incoming::CanUseTool {
                request_id,
                tool,
                input: request.get("input").cloned().unwrap_or(json!({})),
                title: str_field(request, "title").or_else(|| str_field(request, "description")),
            },
            None => Incoming::UnknownControl {
                request_id,
                subtype,
            },
        },
        "hook_callback" => Incoming::Hook {
            request_id,
            callback_id: str_field(request, "callback_id").unwrap_or_default(),
            input: request.get("input").cloned().unwrap_or(Value::Null),
        },
        _ => Incoming::UnknownControl {
            request_id,
            subtype,
        },
    }
}

/// Text and tool calls of an `assistant` message, tool results of a `user`
/// message. User text is skipped: the core records what it sent.
fn messages(v: &Value) -> Vec<AgentMessage> {
    let parent = str_field(v, "parent_tool_use_id");
    let Some(content) = v.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let assistant = v.get("type").and_then(Value::as_str) == Some("assistant");
    content
        .iter()
        .filter_map(|block| {
            let kind = block.get("type").and_then(Value::as_str)?;
            match (assistant, kind) {
                (true, "text") => Some(AgentMessage::AssistantText {
                    text: str_field(block, "text")?,
                    parent: parent.clone(),
                }),
                (true, "tool_use") => Some(AgentMessage::ToolUse {
                    id: str_field(block, "id")?,
                    name: str_field(block, "name")?,
                    input: block.get("input").cloned().unwrap_or(json!({})),
                    parent: parent.clone(),
                }),
                (false, "tool_result") => Some(AgentMessage::ToolResult {
                    tool_use_id: str_field(block, "tool_use_id")?,
                    output: tool_output(block.get("content")),
                    is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
                    parent: parent.clone(),
                }),
                _ => None,
            }
        })
        .collect()
}

/// A tool result's content is a string or a list of blocks.
fn tool_output(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => str_field(b, "text").unwrap_or_default(),
                Some(other) => format!("[{other}]"),
                None => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn turn_end(v: &Value) -> Incoming {
    let tokens = |key: &str| {
        v.pointer(&format!("/usage/{key}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let cost_micro_usd = v
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0)
        .map(|c| (c * 1_000_000.0).round() as u64);
    let usage = TurnUsage {
        input_tokens: tokens("input_tokens")
            + tokens("cache_creation_input_tokens")
            + tokens("cache_read_input_tokens"),
        output_tokens: tokens("output_tokens"),
        cost_micro_usd,
    };
    let failed = v.get("is_error").and_then(Value::as_bool) == Some(true)
        || v.get("subtype")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "success");
    let error = failed.then(|| {
        let interrupted = v
            .get("terminal_reason")
            .and_then(Value::as_str)
            .is_some_and(|r| r.starts_with("aborted"));
        let reason = match v.get("subtype").and_then(Value::as_str) {
            // An interrupt ends the turn with `error_during_execution` and a
            // terminal reason such as `aborted_tools` or `aborted_streaming`.
            _ if interrupted => Some("Interrupted."),
            Some("error_max_budget_usd") => Some("The turn stopped: it reached its budget."),
            Some("error_max_turns") => Some("The turn stopped: it reached its turn limit."),
            _ => None,
        };
        reason
            .map(str::to_owned)
            .or_else(|| str_field(v, "result").filter(|r| !r.is_empty()))
            .unwrap_or_else(|| "The turn failed.".to_owned())
    });
    Incoming::TurnEnd { usage, error }
}

// Lines Ovra writes.

pub(crate) fn initialize() -> Value {
    json!({"type": "control_request", "request_id": INIT_ID, "request": {
        "subtype": "initialize",
        "hooks": {"PreToolUse": [{"matcher": null, "hookCallbackIds": [POLICY_HOOK]}]},
    }})
}

pub(crate) fn user(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

pub(crate) fn interrupt(request_id: &str) -> Value {
    json!({"type": "control_request", "request_id": request_id,
        "request": {"subtype": "interrupt"}})
}

pub(crate) fn success(request_id: &str, response: Value) -> Value {
    json!({"type": "control_response", "response": {
        "subtype": "success", "request_id": request_id, "response": response}})
}

pub(crate) fn error(request_id: &str, message: &str) -> Value {
    json!({"type": "control_response", "response": {
        "subtype": "error", "request_id": request_id, "error": message}})
}

pub(crate) fn allow(input: Value) -> Value {
    json!({"behavior": "allow", "updatedInput": input})
}

pub(crate) fn deny(message: &str) -> Value {
    json!({"behavior": "deny", "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_that_do_not_matter_are_ignored() {
        for line in [
            "not json",
            "{}",
            r#"{"type":"stream_event","event":{}}"#,
            r#"{"type":"rate_limit_event"}"#,
            r#"{"type":"system","subtype":"permission_denied"}"#,
            r#"{"type":"control_response","response":{}}"#,
        ] {
            assert_eq!(parse(line), Incoming::Ignored, "{line}");
        }
    }

    #[test]
    fn tool_results_join_text_blocks() {
        let line = r#"{"type":"user","parent_tool_use_id":"t0","message":{"content":[
            {"type":"tool_result","tool_use_id":"t1","is_error":true,
             "content":[{"type":"text","text":"a"},{"type":"image"}]},
            {"type":"text","text":"subagent prompt"}]}}"#;
        assert_eq!(
            parse(&line.replace('\n', "")),
            Incoming::Messages(vec![AgentMessage::ToolResult {
                tool_use_id: "t1".into(),
                output: "a\n[image]".into(),
                is_error: true,
                parent: Some("t0".into()),
            }])
        );
    }

    #[test]
    fn a_failed_result_carries_its_reason() {
        let line = r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,
            "total_cost_usd":0.0123456,"usage":{"input_tokens":3,"cache_read_input_tokens":10,
            "output_tokens":7}}"#;
        assert_eq!(
            parse(&line.replace('\n', "")),
            Incoming::TurnEnd {
                usage: TurnUsage {
                    input_tokens: 13,
                    output_tokens: 7,
                    cost_micro_usd: Some(12346),
                },
                error: Some("The turn stopped: it reached its budget.".into()),
            }
        );
    }
}
