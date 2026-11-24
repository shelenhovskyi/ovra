use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{MemoryFs, MemoryProjects, MemoryWorktrees, ScriptedAgent, Step};
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, SessionId, SessionMode, TurnUsage,
};
use crate::protocol::orchestration::{OrchestrationEvent, OrchestrationResponse, ShowTarget, Task};
use crate::protocol::permissions::PermissionsEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{ChangeKind, ReviewItem, ReviewResponse, ReviewState};
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::{ToolCaller, ToolRole};

const ROOT: &str = crate::test_path!("/work/a");
const WORKTREES: &str = crate::test_path!("/data/worktrees");

struct Fx {
    core: Core,
    agent: Arc<ScriptedAgent>,
    wt: Arc<MemoryWorktrees>,
    fs: Arc<MemoryFs>,
}

/// A core with project `p1` at [`ROOT`]; every session plays `turns`.
async fn fx(turns: Vec<Vec<Step>>) -> Fx {
    let agent = Arc::new(ScriptedAgent::new(turns));
    let wt = Arc::new(MemoryWorktrees::default());
    let fs = Arc::new(MemoryFs::default());
    wt.put(ROOT, "a.rs", "one\n");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(agent.clone())
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    Fx {
        core,
        agent,
        wt,
        fs,
    }
}

fn say() -> Vec<Vec<Step>> {
    vec![vec![Step::Say("on it")], vec![Step::Say("again")]]
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json).await.unwrap_or_else(|e| panic!("{e}"))
}

fn code<T: std::fmt::Debug>(result: Result<T, ProtocolError>) -> ErrorCode {
    match result {
        Err(e) => e.code,
        Ok(v) => panic!("expected an error, got {v:?}"),
    }
}

fn caller(session: &str, role: ToolRole) -> ToolCaller {
    ToolCaller {
        session: SessionId(session.into()),
        project: ProjectId("p1".into()),
        role,
    }
}

fn sid(id: &str) -> SessionId {
    SessionId(id.into())
}

async fn start_task(core: &Core, max_workers: u32) -> Task {
    let response = ok(
        core,
        json!({"method": "orchestration.start", "params": {
            "project": "p1", "prompt": "split the work",
            "limits": {"max_workers": max_workers, "budget_micro_usd": 1_000_000}}}),
    )
    .await;
    match response {
        Response::Orchestration(OrchestrationResponse::Task(task)) => task,
        other => panic!("unexpected {other:?}"),
    }
}

/// Waits until `session` reaches `want`.
async fn until(rx: &mut broadcast::Receiver<Event>, session: &str, want: AgentStatus) {
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session: s, status, ..
            })) if s.0 == session && status == want => {
                return;
            }
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

async fn reviews(core: &Core) -> Vec<ReviewItem> {
    match ok(
        core,
        json!({"method": "review.list", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Review(ReviewResponse::List(items)) => items,
        other => panic!("unexpected {other:?}"),
    }
}

async fn transcript(core: &Core, session: &str) -> Vec<AgentMessage> {
    core.sessions()
        .find(&sid(session), |s| s.transcript())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_coordinator_reads_the_main_checkout_next_to_a_direct_session() {
    let f = fx(say()).await;
    let task = start_task(&f.core, 3).await;
    assert_eq!(
        (task.id.0.as_str(), task.coordinator.0.as_str()),
        ("t1", "s1")
    );
    let direct = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "fix it"}});
    ok(&f.core, direct.clone()).await;
    // The coordinator took no lock, but the direct session did.
    assert_eq!(code(call(&f.core, direct).await), ErrorCode::Conflict);
    let specs = f
        .agent
        .started
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let coordinator = &specs[0];
    assert_eq!(coordinator.tools, Some(ToolRole::Coordinator));
    assert_eq!(coordinator.mode, SessionMode::Direct);
    assert_eq!(coordinator.cwd, PathBuf::from(ROOT));
    assert_eq!(specs[1].tools, None);

    let isolated = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "isolated", "prompt": "x"}});
    assert_eq!(code(call(&f.core, isolated).await), ErrorCode::Unavailable);
    let zero = json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "x", "limits": {"max_workers": 0, "budget_micro_usd": null}}});
    assert_eq!(code(call(&f.core, zero).await), ErrorCode::BadRequest);
}

#[tokio::test]
async fn workers_start_in_their_own_worktrees_within_the_limit() {
    let f = fx(say()).await;
    start_task(&f.core, 1).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f
        .core
        .spawn_agent(&boss, "fix-login", "fix the login")
        .await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(worker.branch, "ovra/t1/fix-login");
    assert_eq!(
        std::path::Path::new(&worker.worktree),
        std::path::Path::new(WORKTREES)
            .join("p1")
            .join("t1")
            .join("fix-login")
    );
    assert_eq!(f.wt.files(&worker.worktree), f.wt.files(ROOT));
    let spec = f.agent.started.lock().map(|s| s[1].clone());
    let spec = spec.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(spec.mode, SessionMode::Isolated);
    assert_eq!(spec.tools, Some(ToolRole::Worker));
    assert_eq!(spec.cwd, PathBuf::from(&worker.worktree));

    // One worker at a time; names are unique and branch-safe; workers and
    // strangers cannot start workers.
    let full = f.core.spawn_agent(&boss, "other", "x").await;
    assert_eq!(code(full), ErrorCode::Conflict);
    let me = caller("s2", ToolRole::Worker);
    assert_eq!(
        code(f.core.spawn_agent(&me, "sub", "x").await),
        ErrorCode::Forbidden
    );
    let stranger = caller("s9", ToolRole::Coordinator);
    assert_eq!(
        code(f.core.spawn_agent(&stranger, "sub", "x").await),
        ErrorCode::Forbidden
    );

    let mut rx = f.core.subscribe();
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    let again = f.core.spawn_agent(&boss, "fix-login", "x").await;
    assert_eq!(code(again), ErrorCode::Conflict, "the name is taken");
    assert_eq!(
        code(f.core.spawn_agent(&boss, "Bad_Name", "x").await),
        ErrorCode::BadRequest
    );
    assert!(f.core.spawn_agent(&boss, "second", "x").await.is_ok());

    let listed = f.core.list_agents(&boss).unwrap_or_default();
    let names: Vec<_> = listed
        .iter()
        .map(|w| (w.worker.name.as_str(), w.status.is_ended()))
        .collect();
    assert_eq!(names, [("fix-login", true), ("second", false)]);
}

#[tokio::test]
async fn the_budget_stops_new_workers() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    let spent = AgentMessage::TurnEnd {
        usage: TurnUsage {
            cost_micro_usd: Some(1_250_000),
            ..TurnUsage::default()
        },
    };
    let pushed = f.core.sessions().find(&sid("s1"), |s| s.push(spent));
    assert!(pushed.is_ok());
    let boss = caller("s1", ToolRole::Coordinator);
    let err = f.core.spawn_agent(&boss, "w", "x").await.err();
    assert_eq!(
        err.map(|e| e.message).as_deref(),
        Some("the task has spent $1.25, its budget of $1.00")
    );
    let tasks = ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    let Response::Orchestration(OrchestrationResponse::List(tasks)) = tasks else {
        panic!("unexpected {tasks:?}");
    };
    assert_eq!(tasks[0].spent_micro_usd, 1_250_000);
}

#[tokio::test]
async fn ovra_tools_are_decided_by_role_without_asking() {
    let turns = vec![vec![
        Step::Use {
            tool: "mcp__ovra__spawn_agent",
            input: json!({"name": "w", "task": "x"}),
        },
        Step::Use {
            tool: "mcp__ovra__request_review",
            input: json!({"summary": "done"}),
        },
        Step::Use {
            tool: "mcp__ovra__request_review",
            input: json!({"summary": "done", "agent_id": "s1"}),
        },
        Step::Use {
            tool: "mcp__ovra__delete_everything",
            input: json!({}),
        },
    ]];
    let f = fx(turns).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 1).await;
    until(&mut rx, "s1", AgentStatus::Idle).await;
    let boss = caller("s1", ToolRole::Coordinator);
    assert!(f.core.spawn_agent(&boss, "w", "x").await.is_ok());
    until(&mut rx, "s2", AgentStatus::Idle).await;
    ok(
        &f.core,
        json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "plain"}}),
    )
    .await;
    until(&mut rx, "s3", AgentStatus::Idle).await;

    let results = |messages: Vec<AgentMessage>| -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                AgentMessage::ToolResult {
                    output, is_error, ..
                } => Some(if is_error {
                    output
                } else {
                    "allowed".to_owned()
                }),
                _ => None,
            })
            .collect()
    };
    // The coordinator's spawn ran before any worker; review needs a worker.
    insta::assert_debug_snapshot!(
        (
            results(transcript(&f.core, "s1").await),
            results(transcript(&f.core, "s2").await),
            results(transcript(&f.core, "s3").await),
        ),
        @r#"
    (
        [
            "allowed",
            "allowed",
            "allowed",
            "Ovra has no tool named delete_everything",
        ],
        [
            "only the task's coordinator may use spawn_agent",
            "allowed",
            "a worker may ask for a review of its own work only",
            "Ovra has no tool named delete_everything",
        ],
        [
            "this session has no Ovra tools",
            "this session has no Ovra tools",
            "this session has no Ovra tools",
            "this session has no Ovra tools",
        ],
    )
    "#
    );
    // At the limit, the coordinator's next spawn is denied with the reason.
    let denied = crate::orchestration::decide(&f.core, &sid("s1"), "spawn_agent", &json!({}));
    assert_eq!(
        denied.message.as_deref(),
        Some("the task already runs 1 workers, its limit; wait for one to finish or stop one")
    );
    // Workers and coordinators may show files inside their project only.
    let show = |session: &str, path: serde_json::Value| {
        crate::orchestration::decide(
            &f.core,
            &sid(session),
            "open_in_ui",
            &json!({ "path": path }),
        )
        .message
    };
    assert_eq!(
        show("s2", json!("~/.ssh/id_rsa")).as_deref(),
        Some(
            r#"open_in_ui shows files inside the project only: path "~/.ssh/id_rsa" may name a home folder or a variable"#
        )
    );
    assert_eq!(
        show("s1", json!("$HOME/x")).as_deref(),
        Some(
            r#"open_in_ui shows files inside the project only: path "$HOME/x" may name a home folder or a variable"#
        )
    );
    assert_eq!(
        show("s2", json!(null)).as_deref(),
        Some("open_in_ui takes the path as a string")
    );
    assert_eq!(show("s2", json!("src/a.rs")), None);
    assert_eq!(show("s1", json!("src/a.rs")), None);
    // Nobody was asked.
    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(
            event,
            Event::Permissions(PermissionsEvent::Requested(_))
        ));
    }
}

#[tokio::test]
async fn approving_a_worker_applies_its_files_to_the_main_checkout() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    assert!(reviews(&f.core).await.is_empty(), "no changes yet");

    f.wt.put(&worker.worktree, "a.rs", "one\ntwo\n");
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    f.wt.put(ROOT, "b.rs", "mine\n");
    let items = reviews(&f.core).await;
    let files: Vec<_> = items[0]
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert_eq!(
        files,
        [("a.rs", ChangeKind::Modified), ("b.rs", ChangeKind::Added)]
    );
    assert_eq!(items[0].state, ReviewState::Open);

    let diff = ok(
        &f.core,
        json!({"method": "review.diff", "params": {"id": "r1", "path": "a.rs"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Diff { patch, .. }) = diff else {
        panic!("unexpected {diff:?}");
    };
    assert!(patch.contains("+two"));

    let done = ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": "r1", "commit": "Fix a"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done {
        item, conflicts, ..
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    // The person's own b.rs is left alone.
    assert_eq!(
        (item.state, conflicts),
        (ReviewState::Approved, vec!["b.rs".to_owned()])
    );
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("one\ntwo\n")
    );
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("mine\n")
    );
    let committed = f.wt.committed.lock().map(|c| c.clone()).unwrap_or_default();
    assert_eq!(
        committed,
        [(
            PathBuf::from(ROOT),
            vec!["a.rs".to_owned()],
            "Fix a".to_owned()
        )]
    );
    // Only the snapshot the item now compares against is still needed.
    let pruned = f.wt.pruned.lock().map(|p| p.clone()).unwrap_or_default();
    assert_eq!(pruned.len(), 1);
    assert_eq!(pruned[0].0, PathBuf::from(ROOT));
    assert_eq!(pruned[0].1.len(), 1);
    let twice = call(
        &f.core,
        json!({"method": "review.approve", "params": {"id": "r1", "commit": null}}),
    )
    .await;
    assert_eq!(code(twice), ErrorCode::Conflict);
}

#[tokio::test]
async fn a_comment_after_a_reject_continues_the_worker_with_the_lost_diff() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "wrong\n");
    let me = caller("s2", ToolRole::Worker);
    let item = f.core.request_review(&me, None, "changed a.rs").await;
    assert_eq!(
        item.ok().and_then(|i| i.summary).as_deref(),
        Some("changed a.rs")
    );

    let done = ok(
        &f.core,
        json!({"method": "review.reject", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done { item, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(item.state, ReviewState::Rejected);
    let record: Vec<_> = item.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(record, ["a.rs"], "the decided item keeps what was rejected");
    assert_eq!(
        f.wt.files(&worker.worktree),
        f.wt.files(ROOT),
        "back to the start"
    );
    assert_eq!(
        reviews(&f.core).await.len(),
        1,
        "a decided item stays listed"
    );

    ok(
        &f.core,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "keep one"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let sent: Vec<String> = transcript(&f.core, "s2")
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].starts_with("keep one\n\nThe person rejected your changes"));
    assert!(sent[1].contains("+wrong"));
    assert_eq!(reviews(&f.core).await[0].state, ReviewState::Open);

    // A stopped worker resumes on a comment.
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    ok(
        &f.core,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "more"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let specs = f
        .agent
        .started
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let resumed = specs.last().cloned();
    assert_eq!(
        resumed.as_ref().map(|s| s.cwd.clone()),
        Some(PathBuf::from(&worker.worktree))
    );
    assert_eq!(resumed.and_then(|s| s.tools), Some(ToolRole::Worker));
}

#[tokio::test]
async fn a_running_session_holds_its_project() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    assert_eq!(f.fs.watched_ids(), [ProjectId("p1".into())]);
    let client = f.core.connect();
    let open = serde_json::from_value(json!({"method": "projects.open", "params": {"id": "p1"}}));
    let open: Request = open.unwrap_or_else(|e| panic!("{e}"));
    assert!(client.handle(open).await.is_ok());
    drop(client);
    assert_eq!(
        f.fs.watched_ids().len(),
        1,
        "the coordinator still holds it"
    );
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    assert!(f.fs.watched_ids().is_empty());
}

#[tokio::test]
async fn ide_calls_reach_only_the_callers_own_workers() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;

    let first = f.core.read_output(&boss, &worker.session, 0, 2);
    let first = first.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(first.next, 2);
    let rest = f.core.read_output(&boss, &worker.session, first.next, 50);
    assert_eq!(rest.map(|o| o.next).ok(), Some(3), "prompt, text, turn end");
    assert!(
        f.core
            .send_message(&boss, &worker.session, "next")
            .await
            .is_ok()
    );

    // A second task's coordinator cannot reach the first task's worker.
    start_task(&f.core, 3).await;
    let other = caller("s3", ToolRole::Coordinator);
    assert_eq!(
        code(f.core.read_output(&other, &worker.session, 0, 1)),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(f.core.stop_agent(&other, &worker.session)),
        ErrorCode::NotFound
    );
    let me = caller("s2", ToolRole::Worker);
    assert_eq!(code(f.core.list_agents(&me)), ErrorCode::Forbidden);

    let escape = ShowTarget::File {
        path: "../etc/passwd".into(),
        line: None,
    };
    assert!(f.core.open_in_ui(&me, escape).is_err());
    let home = ShowTarget::File {
        path: "~/x".into(),
        line: None,
    };
    assert_eq!(code(f.core.open_in_ui(&me, home)), ErrorCode::Forbidden);
    let file = ShowTarget::File {
        path: "./a.rs".into(),
        line: Some(3),
    };
    assert!(f.core.open_in_ui(&me, file).is_ok());
    loop {
        if let Some(Event::Orchestration(OrchestrationEvent::Show { target, .. })) =
            Core::next_event(&mut rx).await
        {
            assert_eq!(
                target,
                ShowTarget::File {
                    path: "a.rs".into(),
                    line: Some(3)
                }
            );
            break;
        }
    }
}

#[tokio::test]
async fn tasks_and_open_reviews_survive_a_restart() {
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
    start_task(&before, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = before.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    wt.put(&worker.worktree, "a.rs", "two\n");
    let me = caller("s2", ToolRole::Worker);
    assert!(
        before
            .request_review(&me, None, "changed a.rs")
            .await
            .is_ok()
    );
    drop(before);

    let agent = Arc::new(ScriptedAgent::new(say()));
    let after = build(&agent);
    let mut rx = after.subscribe();
    let items = reviews(&after).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].summary.as_deref(), Some("changed a.rs"));
    assert_eq!(items[0].files.len(), 1);
    let tasks = ok(
        &after,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    let Response::Orchestration(OrchestrationResponse::List(tasks)) = tasks else {
        panic!("unexpected {tasks:?}");
    };
    assert_eq!(tasks[0].workers, std::slice::from_ref(&worker));

    // The worker resumes in its worktree, with a worker's tools.
    ok(
        &after,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "also b"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let resumed = agent.started.lock().map(|s| s[0].clone());
    let resumed = resumed.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resumed.cwd, PathBuf::from(&worker.worktree));
    assert_eq!(resumed.tools, Some(ToolRole::Worker));
    ok(
        &after,
        json!({"method": "review.approve", "params": {"id": "r1", "commit": null}}),
    )
    .await;
    assert_eq!(
        wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("two\n")
    );
    assert_eq!(start_task(&after, 3).await.id.0, "t2", "ids go on");
}

#[tokio::test]
async fn after_a_restart_a_direct_review_lists_its_files_and_can_be_kept() {
    let projects = Arc::new(MemoryProjects::default());
    let kept = Arc::new(crate::fakes::MemorySessions::default());
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let build = || {
        Core::builder("test")
            .projects(projects.clone())
            .sessions(kept.clone())
            .agents(Arc::new(ScriptedAgent::new(say())))
            .worktrees(wt.clone())
            .build()
    };
    let before = build();
    ok(
        &before,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let mut rx = before.subscribe();
    ok(
        &before,
        json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "fix it"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Idle).await;
    wt.put(ROOT, "a.rs", "two\n");
    drop(before);

    let after = build();
    let items = reviews(&after).await;
    let files: Vec<_> = items[0]
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.by))
        .collect();
    assert_eq!(
        files,
        [("a.rs", crate::protocol::review::ChangedBy::Outside)]
    );
    let reject = call(
        &after,
        json!({"method": "review.reject", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(
        code(reject),
        ErrorCode::Conflict,
        "nothing is known to be the agent's"
    );
    let done = ok(
        &after,
        json!({"method": "review.approve", "params": {"id": "r1", "commit": null}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done { item, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(item.state, ReviewState::Approved);
    assert_eq!(
        wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("two\n")
    );
}

#[tokio::test]
async fn work_after_a_decision_opens_a_new_item_and_the_old_one_keeps_its_record() {
    let f = fx(say()).await;
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    f.wt.put(&worker.worktree, "a.rs", "two\n");
    ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": "r1", "commit": null}}),
    )
    .await;

    f.wt.put(&worker.worktree, "b.rs", "next\n");
    let items = reviews(&f.core).await;
    let summary: Vec<_> = items
        .iter()
        .map(|i| {
            let files: Vec<_> = i.files.iter().map(|f| f.path.as_str()).collect();
            (i.id.0.as_str(), i.state, files)
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("r2", ReviewState::Open, vec!["b.rs"]),
            ("r1", ReviewState::Approved, vec!["a.rs"]),
        ]
    );
    assert_eq!(
        reviews(&f.core).await.len(),
        2,
        "listing again opens nothing more"
    );
}

#[tokio::test]
async fn snapshots_are_not_pruned_while_a_direct_session_runs() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    ok(
        &f.core,
        json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "fix it"}}),
    )
    .await;
    until(&mut rx, "s3", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "two\n");
    let id = reviews(&f.core)
        .await
        .into_iter()
        .find(|i| i.session == worker.session);
    let id = id.map(|i| i.id.0).unwrap_or_default();
    ok(
        &f.core,
        json!({"method": "review.approve", "params": {"id": id, "commit": null}}),
    )
    .await;
    assert!(f.wt.pruned.lock().map(|p| p.is_empty()).unwrap_or(false));
}

#[test]
fn shown_files_are_surely_inside_the_project() {
    use super::{ide::shown_file, policy::shown_inside};
    let paths = [
        "a.rs",
        "./src/a.rs",
        "src/~notes.md",
        "src/a~b.rs",
        "src/ab:c",
        "a.rs:x",
        ".",
        "./",
        "~",
        "~/x",
        "~user/x",
        "src/~/x",
        "$HOME/x",
        "${HOME}/x",
        "src/$TMP",
        "%USERPROFILE%/x",
        "%APPDATA%",
        "C:a",
        "C:/x",
        "C:\\x",
        "\\x",
        "\\\\server\\share\\x",
        "//server/share/x",
        "/etc/passwd",
        "../x",
        "src/../../x",
        "",
    ];
    let judged: Vec<String> = paths
        .iter()
        .map(|p| match shown_file(p) {
            Ok(shown) => format!("{p} -> {shown}"),
            Err(e) => format!("{p} refused ({:?}): {}", e.code, e.message),
        })
        .collect();
    assert!(shown_inside(&json!({ "path": "~/x" })).is_err());
    assert!(shown_inside(&json!({ "agent_id": "s1" })).is_ok());
    assert!(shown_inside(&json!({ "path": 3 })).is_err());
    insta::assert_snapshot!(judged.join("\n"));
}

#[tokio::test]
async fn stopping_the_coordinator_stops_its_workers() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    for name in ["a", "b"] {
        let spawned = f.core.spawn_agent(&boss, name, "x").await;
        assert!(spawned.is_ok(), "{spawned:?}");
    }
    ok(
        &f.core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    until(&mut rx, "s3", AgentStatus::Stopped).await;
}

#[tokio::test]
async fn a_task_needs_a_repository_before_its_coordinator_starts() {
    let f = fx(say()).await;
    let other = crate::test_path!("/work/plain");
    ok(
        &f.core,
        json!({"method": "projects.add", "params": {"root": other}}),
    )
    .await;
    let start =
        json!({"method": "orchestration.start", "params": {"project": "p2", "prompt": "x"}});
    let err = call(&f.core, start).await.err();
    assert_eq!(err.as_ref().map(|e| e.code), Some(ErrorCode::BadRequest));
    assert!(err.is_some_and(|e| e.message.contains("Use Start for a single session")));
    assert!(
        f.agent
            .started
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn the_coordinator_hears_when_a_worker_asks_for_review_or_stops() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    let me = caller("s2", ToolRole::Worker);
    let asked = f.core.request_review(&me, None, "done").await;
    assert!(asked.is_ok(), "{asked:?}");
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;

    let notes: Vec<String> = transcript(&f.core, "s1")
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text } if text.starts_with("Ovra:") => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes[0].starts_with("Ovra: worker w (agent_id s2) asked for review."));
    assert!(notes[1].starts_with("Ovra: worker w (agent_id s2) stopped."));
    assert!(
        notes.iter().all(|n| !n.contains("done")),
        "the worker's own words stay out"
    );
}

#[tokio::test]
async fn a_worker_waiting_for_review_frees_its_place() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 1).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let full = f.core.spawn_agent(&boss, "v", "x").await;
    assert_eq!(code(full), ErrorCode::Conflict, "the one place is taken");
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    let asked = f
        .core
        .request_review(&caller("s2", ToolRole::Worker), None, "done")
        .await;
    assert!(asked.is_ok(), "{asked:?}");
    assert!(f.core.spawn_agent(&boss, "v", "x").await.is_ok());
}
