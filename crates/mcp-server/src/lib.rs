//! Ovra's IDE tools for agents: an MCP server spoken over JSON-RPC. The
//! runtime passes it each message a session sends to the `ovra` server
//! (`docs/specs/orchestration.md`).
//! Contract: `ovra_core::services::IdeTools`.
//!
//! The tool definitions in `tools.json` are an API other areas and agents
//! depend on: changing them needs a human review. Stub: `initialize` and
//! `tools/list` work; every `tools/call` reports that the tool is not
//! implemented yet (task 3.C).

use ovra_core::Core;
use ovra_core::services::{BoxFuture, IdeTools, ToolCaller, ToolRole};
use serde_json::{Value, json};

/// The tool definitions, MCP `tools/list` shape.
const TOOLS: &str = include_str!("tools.json");

/// MCP protocol revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 2] = ["2025-11-25", "2025-06-18"];

const METHOD_NOT_FOUND: i64 = -32601;

/// Tools a worker may call; the coordinator may call all of them.
const WORKER_TOOLS: [&str; 2] = ["request_review", "open_in_ui"];

/// The IDE tools server.
#[derive(Debug)]
pub struct IdeServer {
    tools: Vec<Value>,
}

impl IdeServer {
    pub fn new() -> Self {
        let tools = serde_json::from_str(TOOLS).unwrap_or_default();
        Self { tools }
    }

    /// The tools `role` may see and call.
    fn tools_for(&self, role: ToolRole) -> Vec<Value> {
        self.tools
            .iter()
            .filter(|tool| {
                role == ToolRole::Coordinator
                    || WORKER_TOOLS.contains(&tool["name"].as_str().unwrap_or(""))
            })
            .cloned()
            .collect()
    }

    fn answer(&self, caller: &ToolCaller, method: &str, params: &Value) -> Result<Value, Value> {
        match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let version = PROTOCOL_VERSIONS
                    .into_iter()
                    .find(|v| *v == asked)
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "ovra", "version": env!("CARGO_PKG_VERSION")},
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.tools_for(caller.role)})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let known = self
                    .tools_for(caller.role)
                    .iter()
                    .any(|t| t["name"] == name);
                let text = if known {
                    format!("{name} is not implemented yet (task 3.C)")
                } else {
                    format!("unknown tool: {name}")
                };
                Ok(json!({"content": [{"type": "text", "text": text}], "isError": true}))
            }
            _ => Err(
                json!({"code": METHOD_NOT_FOUND, "message": format!("unknown method: {method}")}),
            ),
        }
    }
}

impl Default for IdeServer {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeTools for IdeServer {
    fn handle(
        &self,
        _core: Core,
        caller: ToolCaller,
        message: Value,
    ) -> BoxFuture<'_, Option<Value>> {
        Box::pin(async move {
            // Notifications (no id) need no answer.
            let id = message.get("id")?.clone();
            let method = message["method"].as_str().unwrap_or("");
            Some(match self.answer(&caller, method, &message["params"]) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
            })
        })
    }
}

#[cfg(test)]
mod tests;
