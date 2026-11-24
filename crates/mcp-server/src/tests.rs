use std::path::PathBuf;
use std::sync::Arc;

use ovra_core::fakes::{MemoryFs, MemoryProjects, MemoryWorktrees, ScriptedAgent, Step};
use ovra_core::protocol::Request;
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
    let unknown = json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"});
    let reply = ask(ToolRole::Worker, unknown).await;
    assert_eq!(
        reply.map(|r| r["error"]["code"].clone()),
        Some(json!(METHOD_NOT_FOUND))
    );
}

const ROOT: &str = ovra_core::test_path!("/work/a");

/// A core with project `p1` whose task `t1` is led by session `s1`; every
/// session says one thing and waits.
async fn task() -> Core {
    let worktrees = Arc::new(MemoryWorktrees::default());
    worktrees.put(ROOT, "a.rs", "one\n");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(Arc::new(MemoryFs::default()))
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Say("on it")]])))
        .worktrees(worktrees)
        .worktrees_dir(PathBuf::from(ovra_core::test_path!("/data/worktrees")))
        .build();
    for request in [
        json!({"method": "projects.add", "params": {"root": ROOT}}),
        json!({"method": "orchestration.start", "params": {
            "project": "p1", "prompt": "split the work", "limits": {"max_workers": 1}}}),
    ] {
        let request: Request = serde_json::from_value(request).unwrap_or_else(|e| panic!("{e}"));
        core.handle(request).await.unwrap_or_else(|e| panic!("{e}"));
    }
    core
}

/// Calls `tool` as `caller`: the tool's text, and whether it is an error.
async fn tool(core: &Core, caller: ToolCaller, tool: &str, arguments: Value) -> (String, bool) {
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    let reply = IdeServer::new().handle(core.clone(), caller, message).await;
    let result = reply.map(|r| r["result"].clone()).unwrap_or_default();
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    (text, result["isError"] == true)
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[tokio::test]
async fn a_coordinator_runs_a_worker_through_the_tools() {
    let core = task().await;
    let boss = caller(ToolRole::Coordinator);
    let mut rx = core.subscribe();
    let (spawned, error) = tool(
        &core,
        boss.clone(),
        "spawn_agent",
        json!({"task": "fix the login", "name": "fix-login"}),
    )
    .await;
    assert!(!error, "{spawned}");
    let spawned = parsed(&spawned);
    assert_eq!(spawned["branch"], "ovra/t1/fix-login");
    let agent = spawned["agent_id"].clone();
    assert_eq!(agent, "s2");

    let (full, error) = tool(
        &core,
        boss.clone(),
        "spawn_agent",
        json!({"task": "x", "name": "two"}),
    )
    .await;
    assert!(error && full.contains("limit"), "{full}");

    let output = loop {
        let (text, _) = tool(
            &core,
            boss.clone(),
            "read_output",
            json!({"agent_id": agent}),
        )
        .await;
        let output = parsed(&text);
        if output["next"] == 3 {
            break output;
        }
        let _ = Core::next_event(&mut rx).await;
    };
    assert_eq!(
        output["messages"][0],
        json!({"kind": "user_text", "text": "fix the login"})
    );
    assert_eq!(output["messages"][1]["text"], "on it");
    let (later, _) = tool(
        &core,
        boss.clone(),
        "read_output",
        json!({"agent_id": agent, "since": 1, "max_messages": 1}),
    )
    .await;
    let later = parsed(&later);
    assert_eq!(later["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(later["next"], 2);

    let (listed, _) = tool(&core, boss.clone(), "list_agents", json!({})).await;
    let listed = parsed(&listed);
    assert_eq!(listed["agents"][0]["name"], "fix-login");
    assert_eq!(listed["agents"][0]["status"], json!({"state": "idle"}));

    let (reviewed, error) = tool(
        &core,
        boss.clone(),
        "request_review",
        json!({"agent_id": agent, "summary": "login fixed"}),
    )
    .await;
    assert!(!error, "{reviewed}");
    let review = parsed(&reviewed)["review_id"].clone();
    let (listed, _) = tool(&core, boss.clone(), "list_agents", Value::Null).await;
    assert_eq!(parsed(&listed)["agents"][0]["review"]["id"], review);

    assert_eq!(
        tool(
            &core,
            boss.clone(),
            "open_in_ui",
            json!({"review_id": review})
        )
        .await,
        ("Shown in Ovra.".to_owned(), false)
    );
    assert_eq!(
        tool(
            &core,
            boss.clone(),
            "stop_agent",
            json!({"agent_id": agent})
        )
        .await,
        ("Stopped s2.".to_owned(), false)
    );
}

#[tokio::test]
async fn bad_arguments_and_refusals_are_tool_errors() {
    let core = task().await;
    let boss = caller(ToolRole::Coordinator);
    let (text, error) = tool(&core, boss.clone(), "list_agents", json!({"all": true})).await;
    assert!(
        error && text.starts_with("invalid arguments: unknown field `all`"),
        "{text}"
    );
    let (text, error) = tool(&core, boss.clone(), "spawn_agent", json!({"task": "x"})).await;
    assert!(error && text.contains("missing field `name`"), "{text}");
    let (text, error) = tool(
        &core,
        boss.clone(),
        "open_in_ui",
        json!({"path": "a.rs", "agent_id": "s1"}),
    )
    .await;
    assert!(error && text.starts_with("name exactly one"), "{text}");
    let (text, error) = tool(&core, boss.clone(), "open_in_ui", json!({"line": 3})).await;
    assert!(error && text.starts_with("name exactly one"), "{text}");
    let (text, error) = tool(&core, boss.clone(), "open_in_ui", json!({"path": "../x"})).await;
    assert!(error, "{text}");
    let (text, error) = tool(&core, boss, "read_output", json!({"agent_id": "s9"})).await;
    assert!(error && text.contains("not found"), "{text}");

    // A worker reaches only its own work; a coordinator must name the worker.
    let me = caller(ToolRole::Worker);
    let (text, error) = tool(
        &core,
        me,
        "request_review",
        json!({"summary": "done", "agent_id": "s1"}),
    )
    .await;
    assert!(error && text.contains("own work"), "{text}");
}
