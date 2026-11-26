//! Ovra's IDE tools for agents: an MCP server spoken over JSON-RPC. The
//! runtime passes it each message a session sends to the `ovra` server
//! (`docs/specs/orchestration.md`).
//! Contract: `ovra_core::services::IdeTools`.
//!
//! The tool definitions in `tools.json` are an API other areas and agents
//! depend on: changing them needs a human review. Each `tools/call` becomes
//! a call of the core (`Core::spawn_agent` and its siblings), which checks
//! the caller's role and its task's limits.

mod board;
mod browser;
mod calls;
mod servers;

use ovra_core::Core;
use ovra_core::services::{BoxFuture, IdeTools, ToolCaller, ToolRole};
use serde_json::{Value, json};

/// The tool definitions, MCP `tools/list` shape.
const TOOLS: &str = include_str!("tools.json");

/// MCP protocol revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 2] = ["2025-11-25", "2025-06-18"];

const METHOD_NOT_FOUND: i64 = -32601;

/// Tools every role may call; the coordinator may call all of them.
const SHARED_TOOLS: [&str; 25] = [
    "request_review",
    "open_in_ui",
    "claim",
    "release",
    "ask",
    "answer",
    "post",
    "read_board",
    "server_list",
    "server_start",
    "server_stop",
    "server_restart",
    "server_logs",
    "browser_open",
    "browser_navigate",
    "browser_click",
    "browser_type",
    "browser_press_key",
    "browser_wait_for",
    "browser_close",
    "browser_snapshot",
    "browser_console",
    "browser_network",
    "browser_screenshot",
    "browser_inspect",
];

/// Tools only a worker may call: a coordinator has no changes of its own
/// to push.
const WORKER_TOOLS: [&str; 1] = ["push"];

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
                let name = tool["name"].as_str().unwrap_or("");
                match role {
                    ToolRole::Coordinator => !WORKER_TOOLS.contains(&name),
                    ToolRole::Worker => {
                        SHARED_TOOLS.contains(&name) || WORKER_TOOLS.contains(&name)
                    }
                }
            })
            .cloned()
            .collect()
    }

    async fn answer(
        &self,
        core: Core,
        caller: &ToolCaller,
        method: &str,
        params: &Value,
    ) -> Result<Value, Value> {
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
                let arguments = params["arguments"].clone();
                let result = if !known {
                    Err(format!("unknown tool: {name}"))
                } else if let Some(answer) =
                    browser::call(&core, caller, name, arguments.clone()).await
                {
                    answer
                } else {
                    calls::call(&core, caller, name, arguments)
                        .await
                        .map(|text| vec![json!({"type": "text", "text": text})])
                };
                Ok(match result {
                    Ok(content) => json!({"content": content}),
                    Err(text) => {
                        json!({"content": [{"type": "text", "text": text}], "isError": true})
                    }
                })
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
        core: Core,
        caller: ToolCaller,
        message: Value,
    ) -> BoxFuture<'_, Option<Value>> {
        Box::pin(async move {
            // Notifications (no id) need no answer.
            let id = message.get("id")?.clone();
            let method = message["method"].as_str().unwrap_or("");
            Some(
                match self.answer(core, &caller, method, &message["params"]).await {
                    Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
                },
            )
        })
    }
}

#[cfg(test)]
mod tests;
