use ovra_core::protocol::agents::SessionId;
use ovra_core::protocol::projects::ProjectId;

use super::*;

fn caller(role: ToolRole) -> ToolCaller {
    ToolCaller {
        session: SessionId("s1".into()),
        project: ProjectId("p1".into()),
        role,
    }
}

async fn ask(role: ToolRole, message: Value) -> Option<Value> {
    IdeServer::new()
        .handle(Core::new("test"), caller(role), message)
        .await
}

fn names(reply: &Option<Value>) -> Vec<&str> {
    reply.as_ref().map_or_else(Vec::new, |r| {
        r["result"]["tools"]
            .as_array()
            .map(|tools| tools.iter().filter_map(|t| t["name"].as_str()).collect())
            .unwrap_or_default()
    })
}

#[test]
fn every_tool_definition_is_a_closed_object_schema() {
    let tools: Vec<Value> = serde_json::from_str(TOOLS).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(tools.len(), 7);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap_or("");
        assert!(
            !tool["description"].as_str().unwrap_or("").is_empty(),
            "{name}"
        );
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
    }
    insta::assert_json_snapshot!(tools);
}

#[tokio::test]
async fn initialize_lists_and_calls_like_an_mcp_server() {
    let init = ask(
        ToolRole::Worker,
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}}}),
    )
    .await;
    assert_eq!(
        init.as_ref().map(|r| &r["result"]["protocolVersion"]),
        Some(&json!("2025-06-18"))
    );
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert_eq!(ask(ToolRole::Worker, initialized).await, None);

    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    assert_eq!(
        names(&ask(ToolRole::Worker, list.clone()).await),
        WORKER_TOOLS
    );
    assert_eq!(names(&ask(ToolRole::Coordinator, list).await).len(), 7);

    let call = |name: &str| {
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": name, "arguments": {}}})
    };
    let text = |reply: Option<Value>| {
        reply.map(|r| {
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        })
    };
    assert_eq!(
        text(ask(ToolRole::Worker, call("spawn_agent")).await).as_deref(),
        Some("unknown tool: spawn_agent")
    );
    assert_eq!(
        text(ask(ToolRole::Coordinator, call("spawn_agent")).await).as_deref(),
        Some("spawn_agent is not implemented yet (task 3.C)")
    );
    let unknown = json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"});
    let reply = ask(ToolRole::Worker, unknown).await;
    assert_eq!(
        reply.map(|r| r["error"]["code"].clone()),
        Some(json!(METHOD_NOT_FOUND))
    );
}
