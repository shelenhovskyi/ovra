use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{EchoTerminals, MemoryProjects, ScriptedAgent, Step};
use crate::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, AgentsResponse};
use crate::protocol::permissions::{PermissionsEvent, PermissionsResponse};
use crate::protocol::terminals::TerminalsEvent;
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};

fn core_with(agent: ScriptedAgent) -> Core {
    Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .terminals(Arc::new(EchoTerminals::default()))
        .agents(Arc::new(agent))
        .build()
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request =
        serde_json::from_value(json).unwrap_or_else(|e| panic!("bad request: {e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json)
        .await
        .unwrap_or_else(|e| panic!("request failed: {e}"))
}

async fn code(core: &Core, json: serde_json::Value) -> ErrorCode {
    match call(core, json).await {
        Err(e) => e.code,
        Ok(r) => panic!("expected an error, got {r:?}"),
    }
}

async fn project(core: &Core) {
    ok(
        core,
        json!({"method": "projects.add", "params": {"root": "/work/a"}}),
    )
    .await;
}

fn start(prompt: &str) -> serde_json::Value {
    json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": prompt}})
}

/// Reads events until a session reaches `want`, returning every status seen.
async fn until(rx: &mut broadcast::Receiver<Event>, want: AgentStatus) -> Vec<AgentStatus> {
    let mut seen = Vec::new();
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Status { status, .. })) => {
                let done = status == want;
                seen.push(status);
                if done {
                    return seen;
                }
            }
            Some(_) => {}
            None => panic!("core closed while waiting for {want:?}"),
        }
    }
}

async fn transcript(core: &Core) -> Vec<AgentMessage> {
    match ok(
        core,
        json!({"method": "agents.transcript", "params": {"session": "s1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::Transcript(t)) => t,
        other => panic!("unexpected {other:?}"),
    }
}

async fn pending_id(core: &Core) -> String {
    match ok(core, json!({"method": "permissions.pending"})).await {
        Response::Permissions(PermissionsResponse::Pending(p)) if p.len() == 1 => p[0].id.0.clone(),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_session_streams_asks_and_waits_for_the_answer() {
    let core = core_with(ScriptedAgent::new(vec![vec![
        Step::Say("Looking."),
        Step::Use {
            tool: "Bash",
            input: json!({"command": "ls"}),
        },
    ]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("List the files\nthen stop")).await;

    let seen = until(&mut rx, AgentStatus::WaitingForApproval).await;
    assert_eq!(
        seen,
        [
            AgentStatus::Starting,
            AgentStatus::Working,
            AgentStatus::WaitingForApproval
        ]
    );
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}}),
    )
    .await;
    assert_eq!(
        until(&mut rx, AgentStatus::Idle).await,
        [AgentStatus::Working, AgentStatus::Idle]
    );

    insta::assert_json_snapshot!(transcript(&core).await);
    match ok(
        &core,
        json!({"method": "agents.list", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::List(list)) => {
            assert_eq!(list[0].title, "List the files");
            assert_eq!(list[0].status, AgentStatus::Idle);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_denied_tool_reports_the_reason_to_the_agent() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Use {
        tool: "Edit",
        input: json!({"file_path": "a.rs"}),
    }]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("edit")).await;
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    let id = pending_id(&core).await;
    ok(
        &core,
        json!({"method": "permissions.answer",
        "params": {"id": id, "decision": "deny", "message": "not this file"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;
    let result = transcript(&core).await.into_iter().find_map(|m| match m {
        AgentMessage::ToolResult {
            output, is_error, ..
        } => Some((output, is_error)),
        _ => None,
    });
    assert_eq!(result, Some(("not this file".to_owned(), true)));
    let again = json!({"method": "permissions.answer", "params": {"id": id, "decision": "allow"}});
    assert_eq!(code(&core, again).await, ErrorCode::NotFound);
}

#[tokio::test]
async fn each_message_plays_the_next_turn_until_stopped() {
    let core = core_with(ScriptedAgent::new(vec![
        vec![Step::Say("one")],
        vec![Step::Say("two")],
    ]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("go")).await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(
        &core,
        json!({"method": "agents.send", "params": {"session": "s1", "text": "more"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Idle).await;
    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, AgentStatus::Stopped).await;

    let said: Vec<String> = transcript(&core)
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text } | AgentMessage::AssistantText { text, .. } => {
                Some(text)
            }
            _ => None,
        })
        .collect();
    assert_eq!(said, ["go", "one", "more", "two"]);
    let send = json!({"method": "agents.send", "params": {"session": "s1", "text": "late"}});
    assert_eq!(code(&core, send).await, ErrorCode::Conflict);
}

#[tokio::test]
async fn one_direct_session_per_project() {
    let core = core_with(ScriptedAgent::new(vec![vec![Step::Fail("no model")]]));
    project(&core).await;
    let mut rx = core.subscribe();
    ok(&core, start("first")).await;
    assert_eq!(code(&core, start("second")).await, ErrorCode::Conflict);
    until(
        &mut rx,
        AgentStatus::Failed {
            message: "no model".into(),
        },
    )
    .await;
    // The failed session no longer holds the main checkout.
    ok(&core, start("third")).await;

    let isolated = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "isolated", "prompt": "x"}});
    assert_eq!(code(&core, isolated).await, ErrorCode::Unavailable);
    let unknown = json!({"method": "agents.start", "params": {
        "project": "nope", "agent": "claude", "mode": "direct", "prompt": "x"}});
    assert_eq!(code(&core, unknown).await, ErrorCode::NotFound);
}

#[tokio::test]
async fn an_ended_session_drops_its_permission_requests() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    ok(&core, start("x")).await;
    let host = crate::agents::AgentHost::new(
        crate::protocol::agents::SessionId("s1".into()),
        crate::protocol::projects::ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    );
    let mut rx = core.subscribe();
    let asking = tokio::spawn({
        let host = host.clone();
        async move { host.ask("Bash".into(), json!({}), None).await }
    });
    until(&mut rx, AgentStatus::WaitingForApproval).await;
    host.status(AgentStatus::Stopped);
    let answer = asking.await.unwrap_or_else(|e| panic!("ask panicked: {e}"));
    assert_eq!(answer.message.as_deref(), Some("the session ended"));
    let mut resolved = false;
    while let Ok(event) = rx.try_recv() {
        resolved |= matches!(event, Event::Permissions(PermissionsEvent::Resolved { .. }));
    }
    assert!(resolved, "the dropped request was not resolved");
}

#[tokio::test]
async fn terminals_open_in_the_project_root() {
    let core = core_with(ScriptedAgent::default());
    project(&core).await;
    let mut rx = core.subscribe();
    let opened = ok(
        &core,
        json!({"method": "terminals.open",
        "params": {"project": "p1", "size": {"cols": 80, "rows": 24}}}),
    )
    .await;
    insta::assert_json_snapshot!(opened, @r#"
    {
      "type": "terminals.opened",
      "data": {
        "id": "t1"
      }
    }
    "#);
    ok(
        &core,
        json!({"method": "terminals.input", "params": {"id": "t1", "data": "ls\r"}}),
    )
    .await;
    ok(
        &core,
        json!({"method": "terminals.close", "params": {"id": "t1"}}),
    )
    .await;
    let mut events = Vec::new();
    while let Ok(Event::Terminals(e)) = rx.try_recv() {
        events.push(e);
    }
    assert!(
        matches!(&events[..], [TerminalsEvent::Output { data, .. }, TerminalsEvent::Exited { .. }] if data == "ls\r")
    );
    let input = json!({"method": "terminals.input", "params": {"id": "t1", "data": "x"}});
    assert_eq!(code(&core, input).await, ErrorCode::NotFound);
}
