//! ACP v1 (Agent Client Protocol), only the messages Ovra uses: JSON-RPC
//! 2.0, one JSON value per line on the agent's stdin and stdout. A later
//! version is a module of its own; the two never mix on one connection.

use std::path::Path;

use ovra_core::protocol::agents::ToolKind;
use serde::Deserialize;
use serde_json::{Value, json};

/// The version Ovra speaks; an agent that answers another one is refused.
pub(crate) const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC's codes for the requests Ovra refuses.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// `initialize`: Ovra serves no files and no terminals, so the agent reads,
/// writes and runs commands itself, inside its sandbox.
pub(crate) fn initialize(id: u64) -> Value {
    let capabilities = json!({
        "fs": {"readTextFile": false, "writeTextFile": false},
        "terminal": false,
    });
    request(
        id,
        "initialize",
        json!({"protocolVersion": PROTOCOL_VERSION, "clientCapabilities": capabilities}),
    )
}

/// `session/new` in `cwd`, without MCP servers.
pub(crate) fn new_session(id: u64, cwd: &Path) -> Value {
    request(
        id,
        "session/new",
        json!({"cwd": cwd.to_string_lossy(), "mcpServers": []}),
    )
}

/// `session/prompt` with one text block; its response ends the turn.
pub(crate) fn prompt(id: u64, session: &str, text: &str) -> Value {
    request(
        id,
        "session/prompt",
        json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]}),
    )
}

/// `session/cancel`: the agent ends the turn with `cancelled`.
pub(crate) fn cancel(session: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}})
}

/// The answer to a `session/request_permission`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The option the person's decision stands for.
    Selected(String),
    /// The turn was cancelled, or no option says what was decided.
    Cancelled,
}

pub(crate) fn permission(id: &Value, outcome: &Outcome) -> Value {
    let outcome = match outcome {
        Outcome::Selected(option) => json!({"outcome": "selected", "optionId": option}),
        Outcome::Cancelled => json!({"outcome": "cancelled"}),
    };
    json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}})
}

/// An error answer to the agent's request `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    code: i64,
    message: String,
}

pub(crate) fn refuse(id: &Value, refusal: &Refusal) -> Value {
    json!({"jsonrpc": "2.0", "id": id,
        "error": {"code": refusal.code, "message": refusal.message}})
}

/// `initialize`'s result.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Initialized {
    pub(crate) protocol_version: u64,
}

/// `session/new`'s result.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewSession {
    pub(crate) session_id: String,
}

/// `session/prompt`'s result.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptDone {
    pub(crate) stop_reason: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    /// Also a reason Ovra does not know yet.
    #[serde(other)]
    EndTurn,
}

/// What the agent streams in `session/update`.
#[derive(Debug, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub(crate) enum Update {
    AgentMessageChunk {
        content: Content,
    },
    /// Ovra shows no thinking.
    AgentThoughtChunk,
    /// `tool_call` and `tool_call_update`: a call, or what changed of it.
    #[serde(rename = "tool_call", alias = "tool_call_update")]
    Tool(ToolCallFields),
    /// Plans, commands, modes, the person's echoed text, and kinds Ovra
    /// does not know yet.
    #[serde(other)]
    Other,
}

/// A content block; Ovra reads text only.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Content {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

/// What a tool call carries: everything at `tool_call`, the fields that
/// changed at `tool_call_update` and `session/request_permission`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCallFields {
    pub(crate) tool_call_id: String,
    pub(crate) title: Option<String>,
    pub(crate) kind: Option<ToolKind>,
    pub(crate) status: Option<ToolStatus>,
    pub(crate) content: Option<Vec<ToolContent>>,
    pub(crate) locations: Option<Vec<Location>>,
    pub(crate) raw_input: Option<Value>,
    pub(crate) raw_output: Option<Value>,
}

impl ToolCallFields {
    /// Takes the fields `update` carries.
    pub(crate) fn merge(&mut self, update: Self) {
        fn take<T>(field: &mut Option<T>, new: Option<T>) {
            if new.is_some() {
                *field = new;
            }
        }
        take(&mut self.title, update.title);
        take(&mut self.kind, update.kind);
        take(&mut self.status, update.status);
        take(&mut self.content, update.content);
        take(&mut self.locations, update.locations);
        take(&mut self.raw_input, update.raw_input);
        take(&mut self.raw_output, update.raw_output);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolStatus {
    Completed,
    Failed,
    /// `pending`, `in_progress`, and states Ovra does not know yet.
    #[serde(other)]
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ToolContent {
    Content {
        content: Content,
    },
    #[serde(rename_all = "camelCase")]
    Diff {
        new_text: String,
    },
    /// A terminal's output, which Ovra does not serve.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Location {
    pub(crate) path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OptionKind {
    AllowOnce,
    RejectOnce,
    /// `allow_always` and `reject_always`, which Ovra never picks: a lasting
    /// rule is Ovra's own, in the project's policy.
    #[serde(other)]
    Lasting,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionOption {
    pub(crate) option_id: String,
    pub(crate) kind: OptionKind,
}

/// `session/request_permission`'s params.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionRequest {
    pub(crate) tool_call: ToolCallFields,
    pub(crate) options: Vec<PermissionOption>,
}

impl PermissionRequest {
    /// The option that allows (`allow`) or refuses the call once.
    pub(crate) fn outcome(&self, allow: bool) -> Outcome {
        let kind = if allow {
            OptionKind::AllowOnce
        } else {
            OptionKind::RejectOnce
        };
        self.options
            .iter()
            .find(|o| o.kind == kind)
            .map_or(Outcome::Cancelled, |o| {
                Outcome::Selected(o.option_id.clone())
            })
    }
}

/// One line from the agent.
#[derive(Debug)]
pub(crate) enum Incoming {
    /// The answer to Ovra's request `id`: its result or the error's message.
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    Update(Box<Update>),
    Permission {
        id: Value,
        request: Box<PermissionRequest>,
    },
    /// A request Ovra does not serve or cannot read: answered with an error.
    Refused {
        id: Value,
        refusal: Refusal,
    },
    /// Other notifications, and answers Ovra did not ask for.
    Ignored,
}

/// Reads one line; `Err` for a line that is no JSON-RPC message.
pub(crate) fn parse(line: &str) -> Result<Incoming, String> {
    let mut message: Value = serde_json::from_str(line)
        .map_err(|e| format!("the agent wrote a line that is not JSON: {e}"))?;
    if !message.is_object() || message["jsonrpc"] != "2.0" {
        return Err(format!(
            "the agent wrote a line that is not JSON-RPC: {line}"
        ));
    }
    let mut params = message["params"].take();
    let id = message.get("id").filter(|id| !id.is_null()).cloned();
    let Some(method) = message["method"].as_str() else {
        // A response.
        let Some(id) = id.as_ref().and_then(Value::as_u64) else {
            return Ok(Incoming::Ignored);
        };
        let result = match message.get("error") {
            Some(error) => Err(error["message"]
                .as_str()
                .map_or_else(|| error.to_string(), str::to_owned)),
            None => Ok(message["result"].take()),
        };
        return Ok(Incoming::Response { id, result });
    };
    match (id, method) {
        (None, "session/update") => Ok(serde_json::from_value(params["update"].take())
            .map_or(Incoming::Ignored, |u| Incoming::Update(Box::new(u)))),
        (None, _) => Ok(Incoming::Ignored),
        (Some(id), "session/request_permission") => Ok(match serde_json::from_value(params) {
            Ok(request) => Incoming::Permission {
                id,
                request: Box::new(request),
            },
            Err(e) => Incoming::Refused {
                id,
                refusal: Refusal {
                    code: INVALID_PARAMS,
                    message: e.to_string(),
                },
            },
        }),
        (Some(id), method) => Ok(Incoming::Refused {
            id,
            refusal: Refusal {
                code: METHOD_NOT_FOUND,
                message: format!("Ovra does not serve {method}"),
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ovras_lines() {
        let id = json!(7);
        let refusal = Refusal {
            code: METHOD_NOT_FOUND,
            message: "Ovra does not serve fs/read_text_file".into(),
        };
        let lines = [
            initialize(0),
            new_session(1, Path::new("/w")),
            prompt(2, "sess-1", "Fix the bug"),
            cancel("sess-1"),
            permission(&id, &Outcome::Selected("allow".into())),
            permission(&id, &Outcome::Cancelled),
            refuse(&id, &refusal),
        ];
        let lines: Vec<String> = lines.iter().map(Value::to_string).collect();
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn the_agents_lines() {
        let lines = [
            r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}"#,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Authentication required"}}"#,
            r#"{"jsonrpc":"2.0","id":"x","result":{}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hi"}}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"image","data":"","mimeType":"image/png"}}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Hmm"}}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"c1","title":"Edit a.rs","kind":"edit","status":"pending","locations":[{"path":"/w/a.rs","line":3}],"rawInput":{"path":"/w/a.rs"},"content":[{"type":"diff","path":"/w/a.rs","oldText":"x","newText":"y"}]}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"done"}},{"type":"terminal","terminalId":"t"}]}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"c2","kind":"switch_mode","status":"in_progress"}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"plan","entries":[]}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call"}}}"#,
            r#"{"jsonrpc":"2.0","method":"some/notification","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"c1","title":"Run ls","kind":"execute","rawInput":{"command":"ls"}},"options":[{"optionId":"a1","name":"Allow","kind":"allow_once"},{"optionId":"a2","name":"Always","kind":"allow_always"},{"optionId":"r1","name":"Reject","kind":"reject_once"}]}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"session/request_permission","params":{"sessionId":"s"}}"#,
            r#"{"jsonrpc":"2.0","id":"r7","method":"fs/read_text_file","params":{"path":"/etc/passwd"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}"#,
        ];
        let parsed: Vec<String> = lines
            .iter()
            .map(|line| format!("{:?}", parse(line).expect("parses")))
            .collect();
        insta::assert_snapshot!(parsed.join("\n"));
    }

    #[test]
    fn lines_that_are_no_json_rpc_fail() {
        for line in ["not json", "[1]", r#"{"id":1,"result":{}}"#] {
            assert!(parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn stop_reasons_include_unknown_ones() {
        let reasons = [
            "end_turn",
            "max_tokens",
            "max_turn_requests",
            "refusal",
            "cancelled",
            "something_new",
        ];
        let read: Vec<StopReason> = reasons
            .iter()
            .map(|r| {
                let done: PromptDone =
                    serde_json::from_value(json!({"stopReason": r})).expect("reads");
                done.stop_reason
            })
            .collect();
        use StopReason::*;
        assert_eq!(
            read,
            [
                EndTurn,
                MaxTokens,
                MaxTurnRequests,
                Refusal,
                Cancelled,
                EndTurn
            ]
        );
    }

    #[test]
    fn a_permission_is_answered_once_and_never_always() {
        let request = |kinds: &[&str]| PermissionRequest {
            tool_call: ToolCallFields::default(),
            options: kinds
                .iter()
                .map(|kind| PermissionOption {
                    option_id: format!("id-{kind}"),
                    kind: serde_json::from_value(json!(kind)).expect("kind"),
                })
                .collect(),
        };
        let all = request(&["allow_always", "allow_once", "reject_always", "reject_once"]);
        assert_eq!(all.outcome(true), Outcome::Selected("id-allow_once".into()));
        assert_eq!(
            all.outcome(false),
            Outcome::Selected("id-reject_once".into())
        );
        let lasting = request(&["allow_always", "reject_always"]);
        assert_eq!(lasting.outcome(true), Outcome::Cancelled);
        assert_eq!(lasting.outcome(false), Outcome::Cancelled);
    }

    #[test]
    fn an_update_takes_the_fields_it_carries() {
        let mut call: ToolCallFields = serde_json::from_value(json!({
            "toolCallId": "c1", "title": "Run", "kind": "execute", "rawInput": {"command": "ls"}
        }))
        .expect("reads");
        call.merge(
            serde_json::from_value(json!({"toolCallId": "c1", "status": "completed"}))
                .expect("reads"),
        );
        assert_eq!(call.title.as_deref(), Some("Run"));
        assert_eq!(call.status, Some(ToolStatus::Completed));
        assert_eq!(call.raw_input, Some(json!({"command": "ls"})));
    }
}
