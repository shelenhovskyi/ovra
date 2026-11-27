//! Plan first and the message queue (`docs/specs/plan-first.md`).

use super::*;
use crate::agents::BeforeTool;
use crate::protocol::agents::{MessageFrom, QueuedMessage};

fn queue(text: &str) -> serde_json::Value {
    json!({"method": "agents.queue", "params": {"session": "s1", "text": text}})
}

/// Reads events until the session's queue changes, and returns it.
async fn next_queue(rx: &mut broadcast::Receiver<Event>) -> Vec<String> {
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Queue { queue, .. })) => {
                return queue.into_iter().map(|m| m.text).collect();
            }
            Some(_) => {}
            None => panic!("core closed while waiting for the queue"),
        }
    }
}

fn person_said(messages: &[AgentMessage]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, from } if *from == MessageFrom::Person => {
                Some(text.clone())
            }
            AgentMessage::AssistantText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn queued_messages_go_one_by_one_once_a_turn_ends() {
    let core = core_with(ScriptedAgent::new(vec![
        vec![Step::Use {
            tool: "Bash",
            input: json!({"command": "ls"}),
        }],
        vec![Step::Say("two")],
        vec![Step::Say("three")],
    ]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("go")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;

    ok(&core, queue("next")).await;
    assert_eq!(next_queue(&mut rx).await, ["next"]);
    ok(&core, queue("dropped")).await;
    ok(&core, queue("last")).await;
    let unqueue =
        |id: u32| json!({"method": "agents.unqueue", "params": {"session": "s1", "id": id}});
    ok(&core, unqueue(2)).await;
    assert_eq!(
        sessions(&core).await[0].queue,
        [
            QueuedMessage {
                id: 1,
                text: "next".into()
            },
            QueuedMessage {
                id: 3,
                text: "last".into()
            },
        ]
    );
    assert_eq!(code(&core, unqueue(2)).await, ErrorCode::NotFound);

    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
    )
    .await;
    // Each turn's end sends the next one.
    for _ in 0..3 {
        until(&mut rx, AgentStatus::Idle).await;
    }
    assert_eq!(
        person_said(&transcript(&core).await),
        ["go", "next", "two", "last", "three"]
    );
    assert!(sessions(&core).await[0].queue.is_empty());
    assert_eq!(code(&core, unqueue(1)).await, ErrorCode::NotFound);

    // An idle session takes a queued message at once.
    let core = core_with(ScriptedAgent::new(vec![
        vec![Step::Say("one")],
        vec![Step::Say("two")],
    ]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("go")).await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(&core, queue("now")).await;
    until(&mut rx, AgentStatus::Idle).await;
    assert_eq!(
        person_said(&transcript(&core).await),
        ["go", "one", "now", "two"]
    );
}

#[tokio::test]
async fn a_queue_has_room_for_ten_and_ends_with_its_session() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Use {
        tool: "Bash",
        input: json!({"command": "ls"}),
    }]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("go")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    for n in 0..crate::protocol::agents::MAX_QUEUED {
        ok(&core, queue(&format!("m{n}"))).await;
    }
    assert_eq!(code(&core, queue("one more")).await, ErrorCode::Conflict);
    assert_eq!(code(&core, queue("  ")).await, ErrorCode::BadRequest);
    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;
    assert!(sessions(&core).await[0].queue.is_empty());
    assert_eq!(code(&core, queue("late")).await, ErrorCode::Conflict);
    assert_eq!(person_said(&transcript(&core).await), ["go"]);
}

fn plan_first(mode: &str) -> serde_json::Value {
    json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": mode, "prompt": "fix it", "plan_first": true}})
}

#[tokio::test]
async fn plan_first_refuses_changes_and_asks_for_commands_until_approved() {
    let core = core_with(ScriptedAgent::new(vec![
        vec![Step::Say("1. Change a.rs")],
        vec![Step::Say("done")],
    ]));
    project(&core).await;
    ok(
        &core,
        json!({"method": "policy.set", "params": {"project": "p1", "policy": {"rules": [
            {"tool": "Bash", "match": "npm test", "action": "auto"}]}}}),
    )
    .await;
    assert_eq!(
        code(&core, plan_first("research")).await,
        ErrorCode::BadRequest
    );
    let mut rx = core.subscribe();
    ok(&core, plan_first("direct")).await;
    until(&mut rx, AgentStatus::Idle).await;
    assert!(sessions(&core).await[0].planning);
    let first = transcript(&core).await;
    assert!(
        matches!(&first[1], AgentMessage::UserText { text, from: MessageFrom::Ovra }
        if text.starts_with("Plan first:"))
    );

    let host = host(&core);
    let edit = json!({"tool_use_id": "t1", "tool_name": "Write", "tool_input": {"file_path": "a.rs", "content": "x"}});
    assert!(matches!(
        crate::fakes::before_hook(&host, &edit).await,
        BeforeTool::Deny(why) if why.starts_with("Plan first:")
    ));
    let write = crate::fakes::tool("Write", json!({"file_path": "a.rs", "content": "x"}));
    let refused = host.ask(write.clone(), None, None).await;
    assert_eq!(
        refused.decision,
        crate::protocol::permissions::Decision::Deny
    );
    // A command goes to the person, even one a rule runs without asking,
    // and offers no "Always allow".
    let bash =
        json!({"tool_use_id": "t2", "tool_name": "Bash", "tool_input": {"command": "npm test"}});
    assert!(matches!(
        crate::fakes::before_hook(&host, &bash).await,
        BeforeTool::Ask(_)
    ));
    let asking = tokio::spawn({
        let host = host.clone();
        async move {
            host.ask(
                crate::fakes::tool("Bash", json!({"command": "npm test"})),
                None,
                None,
            )
            .await
        }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    match ok(&core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) => {
            assert!(p[0].always.is_none());
            assert!(
                p[0].reason
                    .as_ref()
                    .is_some_and(|r| r.text.starts_with("Plan first:"))
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let approve = json!({"method": "agents.approve_plan", "params": {"session": "s1"}});
    assert_eq!(code(&core, approve.clone()).await, ErrorCode::Conflict);
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer", "params": {"id": id, "decision": "deny"}}),
    )
    .await;
    let _ = asking.await;
    host.status(AgentStatus::Idle);
    until(&mut rx, AgentStatus::Idle).await;

    ok(&core, approve.clone()).await;
    until(&mut rx, AgentStatus::Idle).await;
    assert!(!sessions(&core).await[0].planning);
    assert_eq!(
        crate::fakes::before_hook(&host, &edit).await,
        BeforeTool::Proceed
    );
    let after: Vec<String> = transcript(&core).await[first.len()..]
        .iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, .. } | AgentMessage::AssistantText { text, .. } => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after[0].contains("approved your plan"));
    assert_eq!(after[1], "done");
    assert_eq!(code(&core, approve).await, ErrorCode::Conflict);
}

#[tokio::test]
async fn only_a_solo_task_plans_first() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Say("hi")]]));
    project(&core).await;
    let task = json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "split it", "plan_first": true}});
    assert_eq!(code(&core, task).await, ErrorCode::BadRequest);
}

#[tokio::test]
async fn plan_first_holds_unknown_tools_and_server_starts_and_survives_a_restart() {
    let projects = Arc::new(MemoryProjects::default());
    let kept = Arc::new(MemorySessions::default());
    let first = restartable(
        ScriptedAgent::new(vec![vec![Step::Say("1. Change a.rs")]]),
        &projects,
        &kept,
    );
    project(&first).await;
    let mut rx = first.subscribe();
    ok(&first, plan_first("direct")).await;
    until(&mut rx, AgentStatus::Idle).await;
    let host = host(&first);
    // A tool Ovra does not know goes to the person; Ovra's own tools that
    // run the project's commands are refused.
    let unknown = json!({"tool_use_id": "t1", "tool_name": "NewWriter", "tool_input": {}});
    assert!(matches!(
        crate::fakes::before_hook(&host, &unknown).await,
        BeforeTool::Ask(_)
    ));
    let server = crate::fakes::tool("mcp__ovra__server_start", json!({"name": "web"}));
    assert!(matches!(
        host.before_tool(None, &server).await,
        BeforeTool::Deny(why) if why.starts_with("Plan first:")
    ));
    let read =
        json!({"tool_use_id": "t2", "tool_name": "Read", "tool_input": {"file_path": "a.rs"}});
    assert_eq!(
        crate::fakes::before_hook(&host, &read).await,
        BeforeTool::Proceed
    );
    drop(host);
    drop(first);

    // Kept: after a restart and a resume it still plans.
    let second = restartable(
        ScriptedAgent::new(vec![vec![Step::Say("again")]]),
        &projects,
        &kept,
    );
    assert!(sessions(&second).await[0].planning);
    let mut rx = second.subscribe();
    ok(
        &second,
        json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "go on"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;
    let edit = json!({"tool_use_id": "t3", "tool_name": "Write", "tool_input": {"file_path": "a.rs", "content": "x"}});
    assert!(matches!(
        crate::fakes::before_hook(&super::host(&second), &edit).await,
        BeforeTool::Deny(_)
    ));
}
