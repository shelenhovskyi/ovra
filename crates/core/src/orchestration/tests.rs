use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{MemoryFs, MemoryProjects, MemoryWorktrees, ScriptedAgent, Step};
use crate::protocol::agents::{
    AgentMessage, AgentStatus, AgentsEvent, MessageFrom, SessionId, SessionMode, TurnUsage,
};
use crate::protocol::orchestration::{OrchestrationEvent, OrchestrationResponse, ShowTarget, Task};
use crate::protocol::permissions::PermissionsEvent;
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{ChangeKind, ReviewItem, ReviewResponse, ReviewState};
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::{AgentHost, AgentRuntime, AgentSpec, RunningAgent, ToolCaller, ToolRole};

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

/// A `review.approve` of item `id` with the fingerprint the list shows now.
async fn approval(core: &Core, id: &str, commit: serde_json::Value) -> serde_json::Value {
    let items = reviews(core).await;
    let fingerprint = items
        .iter()
        .find(|i| i.id.0 == id)
        .and_then(|i| i.fingerprint.clone());
    json!({"method": "review.approve", "params": {"id": id, "commit": commit, "fingerprint": fingerprint}})
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

/// A worker's rename moves the file in the main checkout: the old path goes.
/// Rejecting one brings the old file back in the worktree.
#[tokio::test]
async fn a_workers_rename_moves_the_file_when_approved_and_back_when_rejected() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;

    f.wt.delete(&worker.worktree, "a.rs");
    f.wt.put(&worker.worktree, "moved.rs", "one\n");
    let items = reviews(&f.core).await;
    let files: Vec<_> = items[0]
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.kind, f.from.as_deref()))
        .collect();
    assert_eq!(files, [("moved.rs", ChangeKind::Renamed, Some("a.rs"))]);

    let rejected = ok(
        &f.core,
        json!({"method": "review.reject", "params": {"id": "r1"}}),
    )
    .await;
    assert!(matches!(
        rejected,
        Response::Review(ReviewResponse::Done { .. })
    ));
    let back = f.wt.files(&worker.worktree);
    assert_eq!(back.keys().collect::<Vec<_>>(), ["a.rs"]);

    // The worker moves it again; this time the person approves.
    f.wt.delete(&worker.worktree, "a.rs");
    f.wt.put(&worker.worktree, "moved.rs", "one\n");
    let items = reviews(&f.core).await;
    let id = items[0].id.0.clone();
    let done = ok(&f.core, approval(&f.core, &id, json!(null)).await).await;
    let Response::Review(ReviewResponse::Done { conflicts, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert!(conflicts.is_empty(), "{conflicts:?}");
    let main = f.wt.files(ROOT);
    assert_eq!(main.keys().collect::<Vec<_>>(), ["moved.rs"]);
}

#[tokio::test]
async fn approving_a_worker_applies_its_files_to_the_main_checkout() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    assert!(reviews(&f.core).await.is_empty(), "no changes yet");
    until(&mut rx, "s2", AgentStatus::Idle).await;

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
    let Response::Review(ReviewResponse::Diff {
        patch, fingerprint, ..
    }) = diff
    else {
        panic!("unexpected {diff:?}");
    };
    assert!(patch.contains("+two"));
    // The diff says which version of the item it was read from.
    assert!(fingerprint.is_some());
    assert_eq!(fingerprint, items[0].fingerprint);
    f.wt.put(&worker.worktree, "a.rs", "one\ntwo\nthree\n");
    let newer = ok(
        &f.core,
        json!({"method": "review.diff", "params": {"id": "r1", "path": "a.rs"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Diff {
        patch, fingerprint, ..
    }) = newer
    else {
        panic!("unexpected {newer:?}");
    };
    assert!(patch.contains("+three"), "{patch}");
    assert_ne!(fingerprint, items[0].fingerprint);
    f.wt.put(&worker.worktree, "a.rs", "one\ntwo\n");

    let done = ok(&f.core, approval(&f.core, "r1", json!("Fix a")).await).await;
    let Response::Review(ReviewResponse::Done {
        item, conflicts, ..
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    // The person's own b.rs is left alone, and the item says so.
    assert_eq!(
        (item.state, conflicts, item.not_copied.clone()),
        (
            ReviewState::PartlyApproved,
            vec!["b.rs".to_owned()],
            vec!["b.rs".to_owned()]
        )
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
    let twice = call(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    assert_eq!(code(twice), ErrorCode::Conflict);
    // The worker's version of what was not copied stays readable.
    let diff = ok(
        &f.core,
        json!({"method": "review.diff", "params": {"id": "r1", "path": "b.rs"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Diff { patch, .. }) = diff else {
        panic!("unexpected {diff:?}");
    };
    assert!(patch.contains("+new"));
    // Once the person is done with it, the item is approved and keeps the list.
    let done = ok(
        &f.core,
        json!({"method": "review.resolve", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done { item, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(
        (item.state, item.not_copied),
        (ReviewState::Approved, vec!["b.rs".to_owned()])
    );
    let again = call(
        &f.core,
        json!({"method": "review.resolve", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(again), ErrorCode::Conflict);
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
    let sent: Vec<(MessageFrom, String)> = transcript(&f.core, "s2")
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, from } => Some((from, text)),
            _ => None,
        })
        .collect();
    // The person's words are theirs; what Ovra adds is shown as Ovra's.
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[1], (MessageFrom::Person, "keep one".to_owned()));
    assert_eq!(sent[2].0, MessageFrom::Ovra);
    assert!(
        sent[2]
            .1
            .starts_with("Ovra: the person rejected your changes")
    );
    assert!(sent[2].1.contains("+wrong"));
    let listed = reviews(&f.core).await;
    assert_eq!(
        listed.iter().map(|r| r.state).collect::<Vec<_>>(),
        [ReviewState::Rejected],
        "the reject stands; nothing reopens empty"
    );
    // The reworked files open a new item.
    f.wt.put(&worker.worktree, "a.rs", "right\n");
    let listed = reviews(&f.core).await;
    assert_eq!(
        listed
            .iter()
            .map(|r| (r.id.0.as_str(), r.state, r.files.len()))
            .collect::<Vec<_>>(),
        [
            ("r2", ReviewState::Open, 1),
            ("r1", ReviewState::Rejected, 1)
        ]
    );

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
    assert!(f.core.send_message(&boss, &worker.session, "next").is_ok());

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
    ok(&after, approval(&after, "r1", json!(null)).await).await;
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
    let done = ok(&after, approval(&after, "r1", json!(null)).await).await;
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
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "two\n");
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;

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
    ok(&f.core, approval(&f.core, &id, json!(null)).await).await;
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
    let overwrite = f.core.request_review(&boss, Some(&worker.session), "mine");
    assert_eq!(
        code(overwrite.await),
        ErrorCode::Conflict,
        "its summary stays"
    );
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    let resumed = f.core.send_message(&boss, &worker.session, "go on");
    assert_eq!(
        code(resumed),
        ErrorCode::Conflict,
        "a stopped worker stays stopped"
    );

    let notes: Vec<String> = transcript(&f.core, "s1")
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::UserText { text, .. } if text.starts_with("Ovra:") => Some(text),
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
async fn messages_to_agents_say_who_wrote_them() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    until(&mut rx, "s1", AgentStatus::Idle).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "write the notes").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    assert!(
        f.core
            .send_message(&boss, &worker.session, "and the tests")
            .is_ok()
    );
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let me = caller("s2", ToolRole::Worker);
    assert!(f.core.request_review(&me, None, "done").await.is_ok());

    let sent = |messages: Vec<AgentMessage>| -> Vec<(MessageFrom, String)> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                AgentMessage::UserText { text, from } => {
                    Some((from, text.lines().next().unwrap_or("").to_owned()))
                }
                _ => None,
            })
            .collect()
    };
    // The person never wrote the worker's task: the coordinator did.
    assert_eq!(
        sent(transcript(&f.core, "s2").await),
        [
            (MessageFrom::Coordinator, "write the notes".to_owned()),
            (MessageFrom::Coordinator, "and the tests".to_owned()),
        ]
    );
    let boss_got = sent(transcript(&f.core, "s1").await);
    assert_eq!(
        boss_got[0],
        (MessageFrom::Person, "split the work".to_owned())
    );
    assert_eq!(boss_got.last().map(|m| m.0), Some(MessageFrom::Ovra));
    // A transcript saved before authors were recorded never reads as the person's.
    let old: AgentMessage = serde_json::from_value(json!({"kind": "user_text", "text": "x"}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        old,
        AgentMessage::UserText {
            text: "x".into(),
            from: MessageFrom::Unknown
        }
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

#[tokio::test]
async fn a_worker_loads_project_settings_only_if_its_copy_is_the_trusted_one() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    ok(
        &f.core,
        json!({"method": "projects.trust", "params": {"id": "p1", "trusted": true, "fingerprint": ""}}),
    )
    .await;
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let same = f.core.spawn_agent(&boss, "same", "x").await;
    let same = same.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    // Another worker's copy was changed, say by that worker before a resume.
    let changed = std::path::Path::new(WORKTREES)
        .join("p1")
        .join("t1")
        .join("changed");
    let edited = crate::protocol::agents::ProjectAgentConfig {
        fingerprint: "edited".into(),
        items: Vec::new(),
    };
    if let Ok(mut configs) = f.agent.configs.lock() {
        configs.insert(changed, edited);
    }
    assert!(f.core.spawn_agent(&boss, "changed", "x").await.is_ok());
    let trusted: Vec<(PathBuf, bool)> = f
        .agent
        .started
        .lock()
        .map(|s| {
            s.iter()
                .map(|spec| (spec.cwd.clone(), spec.trusted))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(trusted.len(), 3);
    assert!(trusted[0].1, "the coordinator in the main checkout");
    assert_eq!(trusted[1], (PathBuf::from(&same.worktree), true));
    assert!(!trusted[2].1, "the worker whose copy differs");
}

#[tokio::test]
async fn work_is_approved_only_once_its_agent_stopped_changing_it() {
    let bash = Step::Use {
        tool: "Bash",
        input: json!({"command": "ls"}),
    };
    let f = fx(vec![vec![bash]]).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::WaitingForApproval).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    let me = caller("s2", ToolRole::Worker);
    let item = f.core.request_review(&me, None, "done").await;
    let id = item.unwrap_or_else(|e| panic!("{e}")).id;
    let approve = approval(&f.core, &id.0, json!(null)).await;
    let err = call(&f.core, approve.clone()).await.err();
    assert_eq!(err.map(|e| e.code), Some(ErrorCode::Conflict));
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    ok(&f.core, approve).await;
}

#[tokio::test]
async fn a_task_starts_at_most_three_times_its_limit_of_workers() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 1).await;
    let boss = caller("s1", ToolRole::Coordinator);
    for (n, name) in ["a", "b", "c"].into_iter().enumerate() {
        let worker = f.core.spawn_agent(&boss, name, "x").await;
        let worker = worker.unwrap_or_else(|e| panic!("{e}"));
        let session = format!("s{}", n + 2);
        until(&mut rx, &session, AgentStatus::Idle).await;
        assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
        until(&mut rx, &session, AgentStatus::Stopped).await;
    }
    let fourth = f.core.spawn_agent(&boss, "d", "x").await;
    assert_eq!(
        fourth.err().map(|e| e.message),
        Some(
            "the task has started 3 workers, the most it may over its life \
             (3 times its limit of 1); finish with the workers it has"
                .to_owned()
        )
    );
}

#[tokio::test]
async fn an_ended_tasks_settled_worktrees_are_removed_and_the_rest_kept() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let done = f.core.spawn_agent(&boss, "done", "x").await;
    let done = done.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let busy = f.core.spawn_agent(&boss, "busy", "x").await;
    let busy = busy.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s3", AgentStatus::Idle).await;
    f.wt.put(&done.worktree, "a.rs", "approved\n");
    f.wt.put(&busy.worktree, "b.rs", "not decided\n");
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    // While the task runs, nothing is removed.
    let list = json!({"method": "orchestration.list", "params": {"project": "p1"}});
    ok(&f.core, list.clone()).await;
    assert!(f.wt.has(&done.worktree));

    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    ok(&f.core, list.clone()).await;
    assert!(!f.wt.has(&done.worktree), "approved and ended: removed");
    let deleted =
        f.wt.deleted_branches
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default();
    assert_eq!(
        deleted.iter().map(|d| d.1.as_str()).collect::<Vec<_>>(),
        [done.branch.as_str()],
        "its branch goes with it"
    );
    assert!(f.wt.has(&busy.worktree), "undecided changes stay");
    // A removed worker cannot be resumed into a folder that is gone.
    let resumed = call(
        &f.core,
        json!({"method": "review.comment", "params": {"id": "r1", "text": "more"}}),
    )
    .await;
    assert_eq!(code(resumed), ErrorCode::Conflict);
    // Once its changes are decided, the other one goes too.
    ok(
        &f.core,
        json!({"method": "review.reject", "params": {"id": "r2"}}),
    )
    .await;
    assert!(!f.wt.has(&busy.worktree));
}

#[tokio::test]
async fn a_partly_approved_worker_keeps_its_worktree_until_resolved() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    f.wt.put(ROOT, "b.rs", "mine\n");
    // Too many files for the decided record to keep their diffs.
    for n in 0..60 {
        f.wt.put(&worker.worktree, &format!("many/{n}.rs"), "x\n");
    }
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    // What was not copied keeps its diff whatever the size.
    let diff = ok(
        &f.core,
        json!({"method": "review.diff", "params": {"id": "r1", "path": "b.rs"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Diff { patch, .. }) = diff else {
        panic!("unexpected {diff:?}");
    };
    assert!(patch.contains("+new"));
    let other = call(
        &f.core,
        json!({"method": "review.diff", "params": {"id": "r1", "path": "many/1.rs"}}),
    )
    .await;
    assert_eq!(code(other), ErrorCode::NotFound);
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    let list = json!({"method": "orchestration.list", "params": {"project": "p1"}});
    ok(&f.core, list.clone()).await;
    assert!(f.wt.has(&worker.worktree), "b.rs waits for the person");
    ok(
        &f.core,
        json!({"method": "review.resolve", "params": {"id": "r1"}}),
    )
    .await;
    assert!(!f.wt.has(&worker.worktree));
    assert_eq!(reviews(&f.core).await.len(), 1, "the list still works");
    let resumed = call(
        &f.core,
        json!({"method": "agents.resume", "params": {"session": "s2", "prompt": "more"}}),
    )
    .await;
    assert_eq!(code(resumed), ErrorCode::Conflict);
}

#[tokio::test]
async fn a_restart_alone_does_not_end_a_task_or_remove_its_worktrees() {
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
            .worktrees_dir(PathBuf::from(WORKTREES))
            .build()
    };
    let before = build();
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
    drop(before);

    let after = build();
    ok(
        &after,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    assert!(wt.has(&worker.worktree));
}

#[tokio::test]
async fn a_coordinator_that_exits_unasked_leaves_its_task_running() {
    // Turn 1 is what a shutdown looks like: the process goes, nobody asked.
    let f = fx(vec![vec![Step::Say("on it")], vec![Step::Fail("killed")]]).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    until(&mut rx, "s1", AgentStatus::Idle).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    // The note that the worker stopped wakes the coordinator, which fails.
    assert!(f.core.stop_agent(&boss, &worker.session).is_ok());
    loop {
        if let Some(Event::Agents(AgentsEvent::Status {
            session,
            status: AgentStatus::Failed { .. },
            ..
        })) = Core::next_event(&mut rx).await
            && session.0 == "s1"
        {
            break;
        }
    }
    ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    assert!(f.wt.has(&worker.worktree), "the task may be resumed");
    // Until the person stops it.
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    assert!(!f.wt.has(&worker.worktree));
}

#[tokio::test]
async fn a_worktree_git_will_not_remove_is_kept_and_says_why() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    if let Ok(mut refuse) = f.wt.refuse_remove.lock() {
        *refuse = Some("the worktree holds ignored files: .env".into());
    }
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    let listed = ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    let Response::Orchestration(OrchestrationResponse::List(tasks)) = listed else {
        panic!("unexpected {listed:?}");
    };
    assert!(f.wt.has(&worker.worktree));
    assert_eq!(
        tasks[0].workers[0].kept.as_deref(),
        Some("the worktree holds ignored files: .env")
    );
    // It can still be resumed: nothing was removed.
    let resumed = call(
        &f.core,
        json!({"method": "agents.resume", "params": {"session": "s2", "prompt": "more"}}),
    )
    .await;
    assert!(resumed.is_ok(), "{resumed:?}");
}

#[tokio::test]
async fn the_workers_version_can_be_copied_over_mine_and_back() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    f.wt.put(&worker.worktree, "c.rs", "worker\n");
    f.wt.put(ROOT, "b.rs", "mine\n");
    f.wt.put(ROOT, "c.rs", "mine too\n");
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    // The worker changed c.rs after the approve: that is not what was approved.
    f.wt.put(&worker.worktree, "c.rs", "later\n");
    let done = ok(
        &f.core,
        json!({"method": "review.take", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done {
        item,
        conflicts,
        restored,
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(
        (restored, conflicts),
        (vec!["b.rs".to_owned()], vec!["c.rs".to_owned()])
    );
    assert_eq!(
        (item.state, item.not_copied, item.can_undo_take),
        (ReviewState::PartlyApproved, vec!["c.rs".to_owned()], true)
    );
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("new\n")
    );
    assert_eq!(
        f.wt.files(ROOT).get("c.rs").map(String::as_str),
        Some("mine too\n")
    );

    // One replace at a time, and its snapshots are kept for the undo.
    let twice = call(
        &f.core,
        json!({"method": "review.take", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(twice), ErrorCode::Conflict);
    let pruned = f.wt.pruned.lock().map(|p| p.clone()).unwrap_or_default();
    assert_eq!(
        pruned.last().map(|p| p.1.len()),
        Some(3),
        "base, before, after"
    );

    let done = ok(
        &f.core,
        json!({"method": "review.undo_take", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done { item, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("mine\n")
    );
    assert_eq!(
        (item.state, item.not_copied, item.can_undo_take),
        (
            ReviewState::PartlyApproved,
            vec!["b.rs".to_owned(), "c.rs".to_owned()],
            false
        )
    );
    let again = call(
        &f.core,
        json!({"method": "review.undo_take", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(again), ErrorCode::Conflict);
}

#[tokio::test]
async fn an_undone_replace_keeps_what_the_person_wrote_since() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    f.wt.put(ROOT, "b.rs", "mine\n");
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    let done = ok(
        &f.core,
        json!({"method": "review.take", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done { item, .. }) = done else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(
        (item.state, item.can_undo_take),
        (ReviewState::Approved, true)
    );
    assert!(
        f.wt.has(&worker.worktree),
        "kept while the replace can be undone"
    );
    // The person edits the replaced file; the undo leaves that edit alone.
    f.wt.put(ROOT, "b.rs", "edited after\n");
    let done = ok(
        &f.core,
        json!({"method": "review.undo_take", "params": {"id": "r1"}}),
    )
    .await;
    let Response::Review(ReviewResponse::Done {
        item, conflicts, ..
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    assert_eq!(conflicts, ["b.rs"]);
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("edited after\n")
    );
    assert_eq!(
        (item.state, item.can_undo_take),
        (ReviewState::Approved, false)
    );
    // With nothing left to undo, the ended task's worktree goes.
    ok(
        &f.core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    assert!(!f.wt.has(&worker.worktree));
}

#[tokio::test]
async fn new_work_ends_the_undo_of_a_replace() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "b.rs", "new\n");
    f.wt.put(ROOT, "b.rs", "mine\n");
    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    ok(
        &f.core,
        json!({"method": "review.take", "params": {"id": "r1"}}),
    )
    .await;
    f.wt.put(&worker.worktree, "d.rs", "more\n");
    let listed = reviews(&f.core).await;
    assert_eq!(
        listed
            .iter()
            .map(|r| (r.id.0.as_str(), r.can_undo_take))
            .collect::<Vec<_>>(),
        [("r2", false), ("r1", false)]
    );
    let late = call(
        &f.core,
        json!({"method": "review.undo_take", "params": {"id": "r1"}}),
    )
    .await;
    assert_eq!(code(late), ErrorCode::Conflict);
}

#[tokio::test]
async fn approve_lands_only_the_files_the_person_reviewed() {
    let f = fx(say()).await;
    let mut rx = f.core.subscribe();
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x").await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(&mut rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "reviewed\n");
    let seen = approval(&f.core, "r1", json!(null)).await;
    assert!(seen["params"]["fingerprint"].is_string(), "{seen}");

    let blind = json!({"method": "review.approve", "params": {"id": "r1", "commit": null}});
    assert_eq!(
        call(&f.core, blind).await.err().map(|e| e.code),
        Some(ErrorCode::BadRequest)
    );
    f.wt.put(&worker.worktree, "a.rs", "swapped after the look\n");
    let stale = call(&f.core, seen).await.err();
    assert_eq!(stale.as_ref().map(|e| e.code), Some(ErrorCode::Conflict));
    assert!(stale.is_some_and(|e| e.message.contains("review them again")));
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("one\n")
    );

    ok(&f.core, approval(&f.core, "r1", json!(null)).await).await;
    assert_eq!(
        f.wt.files(ROOT).get("a.rs").map(String::as_str),
        Some("swapped after the look\n")
    );
}

/// Plays scripts for the coordinator and its workers; a direct session
/// does nothing by itself, and the test drives its tools through its host.
#[derive(Debug)]
struct WithDirect {
    scripted: ScriptedAgent,
    direct: std::sync::Mutex<Option<AgentHost>>,
}

#[derive(Debug)]
struct Quiet;

impl RunningAgent for Quiet {
    fn send(&self, _: String) -> Result<(), ProtocolError> {
        Ok(())
    }
    fn stop(&self) {}
}

impl AgentRuntime for WithDirect {
    fn start(
        &self,
        spec: AgentSpec,
        host: AgentHost,
    ) -> Result<Box<dyn RunningAgent>, ProtocolError> {
        if spec.mode != SessionMode::Direct {
            return self.scripted.start(spec, host);
        }
        *self
            .direct
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(host);
        Ok(Box::new(Quiet))
    }
}

struct DirectFx {
    core: Core,
    wt: Arc<MemoryWorktrees>,
    runtime: Arc<WithDirect>,
    worktree: String,
}

impl DirectFx {
    /// A task with an idle worker, and a direct session in the main checkout.
    async fn start() -> Self {
        let runtime = Arc::new(WithDirect {
            scripted: ScriptedAgent::new(say()),
            direct: std::sync::Mutex::new(None),
        });
        let wt = Arc::new(MemoryWorktrees::default());
        wt.put(ROOT, "a.rs", "one\n");
        let core = Core::builder("test")
            .projects(Arc::new(MemoryProjects::default()))
            .agents(runtime.clone())
            .worktrees(wt.clone())
            .worktrees_dir(PathBuf::from(WORKTREES))
            .build();
        ok(
            &core,
            json!({"method": "projects.add", "params": {"root": ROOT}}),
        )
        .await;
        let mut rx = core.subscribe();
        start_task(&core, 3).await;
        let boss = caller("s1", ToolRole::Coordinator);
        let worker = core.spawn_agent(&boss, "w", "x").await;
        let worker = worker.unwrap_or_else(|e| panic!("{e}"));
        until(&mut rx, "s2", AgentStatus::Idle).await;
        ok(
            &core,
            json!({"method": "agents.start", "params": {
                "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
        )
        .await;
        Self {
            core,
            wt,
            runtime,
            worktree: worker.worktree,
        }
    }

    /// The direct agent writes `path` in the main checkout with its `Write` tool.
    async fn direct_writes(&self, path: &str, text: &str) {
        let host = self
            .runtime
            .direct
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| panic!("no direct session"));
        let input = json!({"file_path": PathBuf::from(ROOT).join(path), "content": text});
        host.before_tool(&json!({"tool_use_id": path, "tool_name": "Write", "tool_input": input}))
            .await;
        self.wt.put(ROOT, path, text);
        host.message(AgentMessage::ToolResult {
            tool_use_id: path.to_owned(),
            output: String::new(),
            is_error: false,
            parent: None,
        });
        host.tools_settled().await;
    }

    /// The worker's item.
    async fn worker_item(&self) -> ReviewItem {
        reviews(&self.core)
            .await
            .into_iter()
            .find(|i| i.mode == SessionMode::Isolated)
            .unwrap_or_else(|| panic!("no worker item"))
    }
}

#[tokio::test]
async fn approve_refuses_once_the_direct_session_starts_on_the_workers_file() {
    let f = DirectFx::start().await;
    f.wt.put(&f.worktree, "b.rs", "worker\n");
    let seen = f.worker_item().await;
    assert!(!seen.files[0].blocked);
    let approve = json!({"method": "review.approve", "params": {
        "id": seen.id.0, "commit": null, "fingerprint": seen.fingerprint}});

    // The direct agent writes the same file: it now waits, and an approve
    // sent for the list without the wait would copy it over the agent's.
    f.direct_writes("b.rs", "direct\n").await;
    assert_eq!(code(call(&f.core, approve).await), ErrorCode::Conflict);
    let now = f.worker_item().await;
    assert!(now.files[0].blocked);
    assert_ne!(now.fingerprint, seen.fingerprint);
    assert_eq!(
        f.wt.files(ROOT).get("b.rs").map(String::as_str),
        Some("direct\n")
    );
}

#[tokio::test]
async fn a_workers_rename_waits_when_the_direct_session_edits_its_old_path() {
    let f = DirectFx::start().await;
    f.wt.delete(&f.worktree, "a.rs");
    f.wt.put(&f.worktree, "moved.rs", "one\n");
    f.direct_writes("a.rs", "direct\n").await;
    let item = f.worker_item().await;
    assert_eq!(
        item.files
            .iter()
            .map(|f| (f.path.as_str(), f.from.as_deref(), f.blocked))
            .collect::<Vec<_>>(),
        [("moved.rs", Some("a.rs"), true)]
    );
    let id = item.id.0.clone();
    let done = ok(&f.core, approval(&f.core, &id, json!(null)).await).await;
    let Response::Review(ReviewResponse::Done {
        item, conflicts, ..
    }) = done
    else {
        panic!("unexpected {done:?}");
    };
    // Both paths are left, so a later replace moves the file, not copies it.
    assert_eq!(conflicts, ["a.rs", "moved.rs"]);
    assert_eq!(item.not_copied, ["a.rs", "moved.rs"]);
    let main = f.wt.files(ROOT);
    assert_eq!(main.keys().collect::<Vec<_>>(), ["a.rs"]);
    assert_eq!(main.get("a.rs").map(String::as_str), Some("direct\n"));
}
