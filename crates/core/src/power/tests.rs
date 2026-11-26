use std::sync::Arc;

use serde_json::json;

use crate::Core;
use crate::agents::AgentHost;
use crate::fakes::{FakeKeepAwake, MemoryProjects, ScriptedAgent};
use crate::protocol::agents::{AgentStatus, SessionId};
use crate::protocol::power::{KeepAwakeMode, PowerEvent, PowerResponse, PowerState};
use crate::protocol::projects::ProjectId;
use crate::protocol::{Event, Request, Response};

fn core_with(keep: Arc<FakeKeepAwake>) -> Core {
    Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::default()))
        .keep_awake(keep)
        .build()
}

async fn call(core: &Core, json: serde_json::Value) -> PowerState {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    match core.handle(request).await {
        Ok(Response::Power(PowerResponse::State(state))) => state,
        other => panic!("unexpected {other:?}"),
    }
}

async fn set(core: &Core, mode: &str) -> PowerState {
    call(
        core,
        json!({"method": "power.set_mode", "params": {"mode": mode}}),
    )
    .await
}

fn changes(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<PowerState> {
    let mut seen = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let Event::Power(PowerEvent::Changed(state)) = event {
            seen.push(state);
        }
    }
    seen
}

fn host(core: &Core) -> AgentHost {
    AgentHost::new(
        SessionId("s1".into()),
        ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    )
}

#[tokio::test]
async fn always_holds_until_turned_off_and_says_so_once_per_change() {
    let keep = Arc::new(FakeKeepAwake::default());
    let core = core_with(keep.clone());
    let off = call(&core, json!({"method": "power.get"})).await;
    assert_eq!((off.mode, off.awake), (KeepAwakeMode::Off, false));

    let mut rx = core.subscribe();
    assert!(set(&core, "always").await.awake);
    assert!(set(&core, "always").await.awake);
    assert!(!set(&core, "off").await.awake);
    assert_eq!(*keep.changes.lock().expect("lock"), [true, false]);
    let seen: Vec<_> = changes(&mut rx).iter().map(|s| s.awake).collect();
    assert_eq!(seen, [true, false]);
}

#[tokio::test]
async fn while_agents_work_follows_the_sessions() {
    let keep = Arc::new(FakeKeepAwake::default());
    let core = core_with(keep.clone());
    assert!(!set(&core, "while_agents_work").await.awake);

    core.handle(
        serde_json::from_value(
            json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
        )
        .expect("request"),
    )
    .await
    .expect("project");
    core.handle(
        serde_json::from_value(json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "x"}}))
        .expect("request"),
    )
    .await
    .expect("start");
    // Starting counts as working.
    assert!(keep.awake());

    let host = host(&core);
    host.status(AgentStatus::Working);
    assert!(keep.awake());
    host.status(AgentStatus::WaitingForApproval);
    assert!(
        keep.awake(),
        "a session waiting for a person keeps it awake"
    );
    host.status(AgentStatus::Working);
    host.status(AgentStatus::Idle);
    assert!(!keep.awake(), "an idle session lets it sleep");
    host.status(AgentStatus::Working);
    host.status(AgentStatus::Stopped);
    assert!(!keep.awake());

    // Turning the mode off while agents work releases at once.
    assert!(!set(&core, "off").await.awake);
}

#[tokio::test]
async fn a_system_that_cannot_hold_reports_the_problem_and_keeps_the_mode() {
    let core = core_with(Arc::new(FakeKeepAwake::failing()));
    let state = set(&core, "always").await;
    assert_eq!(state.mode, KeepAwakeMode::Always);
    assert!(!state.awake);
    assert!(state.problem.is_some());
    // Off needs no hold, so the problem goes away.
    assert_eq!(set(&core, "off").await.problem, None);
}
