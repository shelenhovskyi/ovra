use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{MemoryFs, MemoryProjects, ScriptedAgent, Step};
use crate::protocol::agents::{AgentStatus, AgentsEvent};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};

const ROOT: &str = "/work/a";

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json).await.unwrap_or_else(|e| panic!("{e}"))
}

/// Waits until `session` reaches `want`.
async fn until(rx: &mut broadcast::Receiver<Event>, session: &str, want: AgentStatus) {
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session: s, status, ..
            })) if s.0 == session && status == want => return,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

#[tokio::test]
async fn a_running_session_holds_its_project() {
    let fs = Arc::new(MemoryFs::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Say("hi")]])))
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let mut rx = core.subscribe();
    ok(
        &core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "fix it"}}),
    )
    .await;
    assert_eq!(fs.watched_ids(), [ProjectId("p1".into())]);
    let client = core.connect();
    let open = serde_json::from_value(json!({"method": "projects.open", "params": {"id": "p1"}}));
    let open: Request = open.unwrap_or_else(|e| panic!("{e}"));
    assert!(client.handle(open).await.is_ok());
    drop(client);
    assert_eq!(fs.watched_ids().len(), 1, "the session still holds it");
    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    assert!(fs.watched_ids().is_empty());
}

#[tokio::test]
async fn review_and_orchestration_say_they_are_not_ready() {
    let core = Core::new("test");
    for json in [
        json!({"method": "review.list", "params": {"project": "p1"}}),
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    ] {
        let code = call(&core, json).await.err().map(|e| e.code);
        assert_eq!(code, Some(ErrorCode::Unavailable));
    }
}
