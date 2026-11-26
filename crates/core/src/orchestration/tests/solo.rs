//! One agent in its own copy: a task with one worker and no coordinator
//! (`docs/specs/more-agents.md`).

use super::*;
use crate::protocol::agents::{AgentKind, Capabilities, ProviderInfo};

fn solo(agent: &str, prompt: &str) -> serde_json::Value {
    json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": prompt, "agent": agent, "solo": true}})
}

async fn start_solo(core: &Core, agent: &str) -> Task {
    match ok(core, solo(agent, "fix the typo")).await {
        Response::Orchestration(OrchestrationResponse::Task(task)) => task,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn one_agent_works_in_its_own_worktree_and_is_reviewed_like_a_worker() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    let task = start_solo(&f.core, "claude").await;
    assert_eq!(task.coordinator, None);
    let [worker] = task.workers.as_slice() else {
        panic!("one worker: {task:?}");
    };
    assert_eq!(worker.name, "agent");
    assert_eq!(worker.branch, format!("ovra/{}/agent", task.id.0));
    until(&mut rx, &worker.session.0, AgentStatus::Idle).await;
    // The person's prompt, as the person's.
    let first = transcript(&f.core, &worker.session.0).await;
    assert!(
        matches!(first.first(), Some(AgentMessage::UserText { text, from: MessageFrom::Person }) if text == "fix the typo"),
        "{first:?}"
    );
    let started = f
        .agent
        .started
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    assert_eq!(started.len(), 1, "no coordinator started");
    assert_eq!(started[0].mode, SessionMode::Isolated);
    assert_eq!(started[0].cwd, PathBuf::from(&worker.worktree));

    f.wt.put(&worker.worktree, "a.rs", "one\ntwo\n");
    let items = reviews(&f.core).await;
    assert_eq!(items.len(), 1);
    let approve = approval(&f.core, &items[0].id.0, json!(null)).await;
    ok(&f.core, approve).await;
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("one\ntwo\n")
    );

    // Listed once, with no coordinator to tell.
    let Response::Orchestration(OrchestrationResponse::List(tasks)) = ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no list");
    };
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].coordinator, None);
}

#[tokio::test]
async fn stopping_its_task_stops_the_agent() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    let task = start_solo(&f.core, "claude").await;
    let session = task.workers[0].session.clone();
    until(&mut rx, &session.0, AgentStatus::Idle).await;
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": task.id.0}}),
    )
    .await;
    until(&mut rx, &session.0, AgentStatus::Stopped).await;
}

#[tokio::test]
async fn a_provider_without_an_enforced_policy_runs_in_its_own_copy() {
    let gemini = ProviderInfo {
        id: AgentKind::Gemini,
        name: "Gemini".into(),
        capabilities: Capabilities {
            approvals: true,
            ..Capabilities::default()
        },
    };
    let agent = Arc::new(ScriptedAgent::new(say()).as_provider(gemini));
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(agent.clone())
        .worktrees(wt)
        .worktrees_dir(PathBuf::from(WORKTREES))
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let direct = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "gemini", "mode": "direct", "prompt": "x"}});
    assert_eq!(code(call(&core, direct).await), ErrorCode::Unavailable);
    let plan = json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "x", "agent": "gemini"}});
    assert_eq!(code(call(&core, plan).await), ErrorCode::Unavailable);
    let task = start_solo(&core, "gemini").await;
    let started = agent.started.lock().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].agent, AgentKind::Gemini);
    assert_eq!(started[0].mode, SessionMode::Isolated);
    // Its IDE tools wait for the bridge: it runs without them.
    assert_eq!(started[0].tools, None);
    assert_eq!(task.workers.len(), 1);
}

#[tokio::test]
async fn one_agent_needs_a_repository_and_leaves_no_task_when_it_cannot_start() {
    let f = fx(say()).await;
    let other = crate::test_path!("/work/plain");
    ok(
        &f.core,
        json!({"method": "projects.add", "params": {"root": other}}),
    )
    .await;
    let plain = json!({"method": "orchestration.start", "params": {
        "project": "p2", "prompt": "x", "solo": true}});
    let err = call(&f.core, plain).await.err();
    assert!(err.is_some_and(|e| e.message.contains("One agent, own copy")));
    // An unknown provider is refused once its worktree was made: nothing stays.
    assert_eq!(
        code(call(&f.core, solo("codex", "x")).await),
        ErrorCode::Unavailable
    );
    let Response::Orchestration(OrchestrationResponse::List(tasks)) = ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no list");
    };
    assert!(tasks.is_empty(), "{tasks:?}");
}
