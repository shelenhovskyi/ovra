//! The router through the core: each provider's sessions reach its own
//! runtime, and the core refuses what a provider cannot do.

use std::sync::Arc;

use serde_json::json;

use super::AgentRuntimes;
use crate::Core;
use crate::fakes::{MemoryProjects, ScriptedAgent, Step};
use crate::protocol::agents::{AgentKind, AgentsResponse, Capabilities, ProviderInfo, SetupState};
use crate::protocol::{ErrorCode, ProtocolError, Request, Response};

/// An ACP agent as `ovra-acp-client` reports one: no enforced policy, no
/// IDE tools, no accounts, no research.
fn gemini() -> ProviderInfo {
    ProviderInfo {
        id: AgentKind::Gemini,
        name: "Gemini".to_owned(),
        capabilities: Capabilities {
            approvals: true,
            interrupt: true,
            text_deltas: true,
            ..Capabilities::default()
        },
    }
}

struct Fx {
    core: Core,
    claude: Arc<ScriptedAgent>,
    gemini: Arc<ScriptedAgent>,
}

async fn fx() -> Fx {
    let claude = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("hi")]]));
    let gemini = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("hello")]]).as_provider(gemini()));
    let runtimes = AgentRuntimes::new(vec![claude.clone(), gemini.clone()]);
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(runtimes))
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    Fx {
        core,
        claude,
        gemini,
    }
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

fn start(agent: &str, mode: &str) -> serde_json::Value {
    json!({"method": "agents.start", "params": {
        "project": "p1", "agent": agent, "mode": mode, "prompt": "fix it"}})
}

fn refused(result: Result<Response, ProtocolError>) -> ProtocolError {
    match result {
        Err(e) => e,
        Ok(r) => panic!("expected a refusal, got {r:?}"),
    }
}

#[tokio::test]
async fn providers_are_every_runtimes_in_order() {
    let f = fx().await;
    let Ok(Response::Agents(AgentsResponse::Providers(list))) =
        call(&f.core, json!({"method": "agents.providers"})).await
    else {
        panic!("no providers");
    };
    let ids: Vec<_> = list.iter().map(|p| p.id).collect();
    assert_eq!(ids, [AgentKind::Claude, AgentKind::Gemini]);
}

#[tokio::test]
async fn each_session_starts_in_its_providers_runtime() {
    let f = fx().await;
    call(&f.core, start("claude", "direct"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.claude.started.lock().map(|s| s.len()).unwrap_or(0), 1);
    assert_eq!(f.gemini.started.lock().map(|s| s.len()).unwrap_or(0), 0);
}

#[tokio::test]
async fn a_provider_without_an_enforced_policy_works_only_in_its_own_copy() {
    let f = fx().await;
    let err = refused(call(&f.core, start("gemini", "direct")).await);
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err.message.contains("own copy"), "{}", err.message);
    let err = refused(call(&f.core, start("gemini", "research")).await);
    assert!(err.message.contains("research"), "{}", err.message);
    assert!(
        f.gemini
            .started
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or(false)
    );
    // Nothing was left behind: the project's direct place is free.
    call(&f.core, start("claude", "direct"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
}

#[tokio::test]
async fn an_unknown_provider_is_refused_and_its_setup_is_broken() {
    let claude = Arc::new(ScriptedAgent::new(Vec::new()));
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(AgentRuntimes::new(vec![claude])))
        .build();
    call(
        &core,
        json!({"method": "projects.add", "params": {"root": crate::test_path!("/work/a")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let err = refused(call(&core, start("codex", "direct")).await);
    assert_eq!(err.code, ErrorCode::Unavailable);
    let Ok(Response::Agents(AgentsResponse::Setup(setup))) = call(
        &core,
        json!({"method": "agents.setup", "params": {"agent": "codex"}}),
    )
    .await
    else {
        panic!("no setup");
    };
    assert!(
        matches!(setup.state, SetupState::Broken { .. }),
        "{setup:?}"
    );
}

#[tokio::test]
async fn a_chosen_path_is_kept_per_provider() {
    let f = fx().await;
    call(
        &f.core,
        json!({"method": "agents.set_path", "params": {
            "agent": "gemini", "path": crate::test_path!("/opt/gemini")}}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let Ok(Response::Agents(AgentsResponse::Setup(setup))) = call(
        &f.core,
        json!({"method": "agents.setup", "params": {"agent": "claude"}}),
    )
    .await
    else {
        panic!("no setup");
    };
    assert_eq!(setup.path, None);
    let Ok(Response::Agents(AgentsResponse::Setup(setup))) = call(
        &f.core,
        json!({"method": "agents.setup", "params": {"agent": "gemini"}}),
    )
    .await
    else {
        panic!("no setup");
    };
    assert_eq!(
        setup.path.as_deref(),
        Some(crate::test_path!("/opt/gemini"))
    );
}
