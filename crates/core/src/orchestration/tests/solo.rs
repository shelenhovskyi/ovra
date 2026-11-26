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

#[tokio::test]
async fn after_a_restart_the_agent_resumes_in_its_own_copy_as_a_worker() {
    let projects = Arc::new(MemoryProjects::default());
    let kept = Arc::new(crate::fakes::MemorySessions::default());
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let build = |agent: &Arc<ScriptedAgent>| {
        Core::builder("test")
            .projects(projects.clone())
            .sessions(kept.clone())
            .agents(agent.clone())
            .worktrees(wt.clone())
            .worktrees_dir(PathBuf::from(WORKTREES))
            .build()
    };
    let before = build(&Arc::new(ScriptedAgent::new(say())));
    ok(
        &before,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let mut rx = before.subscribe();
    let task = start_solo(&before, "claude").await;
    let worker = task.workers[0].clone();
    until(&mut rx, &worker.session.0, AgentStatus::Idle).await;
    drop(before);

    let agent = Arc::new(ScriptedAgent::new(say()));
    let after = build(&agent);
    let mut rx = after.subscribe();
    let send = json!({"method": "agents.resume", "params": {
        "session": worker.session.0, "prompt": "go on"}});
    ok(&after, send).await;
    until(&mut rx, &worker.session.0, AgentStatus::Idle).await;
    let resumed = agent.started.lock().map(|s| s[0].clone());
    let resumed = resumed.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resumed.cwd, PathBuf::from(&worker.worktree));
    assert_eq!(resumed.tools, Some(ToolRole::Worker));
}

#[tokio::test]
async fn one_agent_runs_with_the_projects_account() {
    let f = fx(say()).await;
    let work = crate::test_path!("/home/me/.claude-work");
    ok(
        &f.core,
        json!({"method": "agents.save_account", "params": {"account": {
            "id": "", "name": "Work", "config_dir": work}}}),
    )
    .await;
    ok(
        &f.core,
        json!({"method": "agents.set_project_account", "params": {"project": "p1", "account": "account-1"}}),
    )
    .await;
    start_solo(&f.core, "claude").await;
    let started = f.agent.started.lock().map(|s| s.clone());
    let started = started.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        started[0].config_dir.as_deref(),
        Some(std::path::Path::new(work))
    );
}

#[tokio::test]
async fn stopping_while_its_copy_is_made_keeps_the_agent_from_starting() {
    let f = fx(say()).await;
    let (open, gate) = tokio::sync::oneshot::channel();
    *f.wt.create_gate.lock().unwrap_or_else(|e| e.into_inner()) = Some(gate);
    let core = f.core.clone();
    let starting = tokio::spawn(async move { call(&core, solo("claude", "x")).await });
    // The task is listed while its copy is made.
    let id = loop {
        tokio::task::yield_now().await;
        let list = ok(
            &f.core,
            json!({"method": "orchestration.list", "params": {"project": "p1"}}),
        )
        .await;
        if let Response::Orchestration(OrchestrationResponse::List(tasks)) = list
            && let Some(task) = tasks.first()
        {
            break task.id.0.clone();
        }
    };
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": id}}),
    )
    .await;
    let _ = open.send(());
    let task = starting.await.unwrap_or_else(|e| panic!("{e}"));
    let Ok(Response::Orchestration(OrchestrationResponse::Task(task))) = task else {
        panic!("unexpected {task:?}");
    };
    let started = f.agent.started.lock().map(|s| s.len()).unwrap_or(0);
    assert_eq!(started, 0, "the stopped agent never ran");
    let session = task.workers[0].session.clone();
    let stopped = f
        .core
        .sessions()
        .find(&session, |s| s.status().is_ended())
        .unwrap_or(false);
    assert!(stopped, "its session ended");
}

#[tokio::test]
async fn an_agent_in_its_own_copy_gets_the_images_once_its_worktree_is_ready() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    f.fs.put_bytes(
        std::path::Path::new(ROOT),
        ".ovra-attachments/shot.png",
        b"\x89PNG\r\n\x1a\nxy",
    );
    let start = json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "what is wrong", "agent": "claude", "solo": true,
        "context": [{"kind": "image", "path": ".ovra-attachments/shot.png"}]}});
    let Response::Orchestration(OrchestrationResponse::Task(task)) = ok(&f.core, start).await
    else {
        panic!("no task");
    };
    until(&mut rx, &task.workers[0].session.0, AgentStatus::Idle).await;
    let started = f
        .agent
        .started
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].mode, SessionMode::Isolated);
    assert_eq!(
        started[0]
            .images
            .iter()
            .map(|i| i.media_type)
            .collect::<Vec<_>>(),
        ["image/png"]
    );
}
