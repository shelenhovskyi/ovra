//! Requests a server makes of the client. The core answers them itself
//! (`docs/specs/lsp.md`, "Requests from the server"): the UI never sees
//! them, so a server cannot make Ovra edit files, open documents or
//! follow links.

use serde_json::{Value, json};

/// `METHOD_NOT_FOUND` in JSON-RPC.
const METHOD_NOT_FOUND: i64 = -32601;

/// The reply to the request `message` (it has an `id` and a `method`).
/// `root_uri` and `name` describe the project's only workspace folder.
pub(crate) fn reply(message: &Value, root_uri: &str, name: &str) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "workspace/configuration" => {
            let items = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; items])
        }
        "workspace/applyEdit" => json!({
            "applied": false,
            "failureReason": "Ovra does not apply edits a language server asks for",
        }),
        "window/showDocument" => json!({ "success": false }),
        "workspace/workspaceFolders" => json!([{ "uri": root_uri, "name": name }]),
        "window/showMessageRequest"
        | "window/workDoneProgress/create"
        | "client/registerCapability"
        | "client/unregisterCapability" => Value::Null,
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": METHOD_NOT_FOUND, "message": format!("{method} is not supported") },
            });
        }
    };
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
