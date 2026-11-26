//! The project's check before review (`docs/specs/review-check.md`).

use std::sync::atomic::Ordering;

use super::*;
use crate::fakes::FakeDevServers;
use crate::protocol::review::{ReviewCheck, ReviewCheckState};
use crate::services::SetupEnd;

struct Checked {
    core: Core,
    dev: Arc<FakeDevServers>,
    fs: Arc<MemoryFs>,
    wt: Arc<MemoryWorktrees>,
}

/// A core with project `p1` and fake pods; `check` is the text of
/// `.ovra/check.json` in the main checkout, if any.
async fn checked(check: Option<&str>) -> Checked {
    checked_with(check, ScriptedAgent::new(say())).await
}

async fn checked_with(check: Option<&str>, agent: ScriptedAgent) -> Checked {
    let wt = Arc::new(MemoryWorktrees::default());
    let fs = Arc::new(MemoryFs::default());
    let dev = Arc::new(FakeDevServers::default());
    wt.put(ROOT, "a.rs", "one\n");
    if let Some(text) = check {
        fs.put(std::path::Path::new(ROOT), ".ovra/check.json", text);
    }
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(Arc::new(agent))
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .dev_servers(dev.clone())
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    Checked { core, dev, fs, wt }
}

/// Starts a task with one worker, `s2`, that changed a file and is idle.
async fn worker(f: &Checked, rx: &mut broadcast::Receiver<Event>) -> ToolCaller {
    start_task(&f.core, 3).await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "x", false).await;
    let worker = worker.unwrap_or_else(|e| panic!("{e}"));
    until(rx, "s2", AgentStatus::Idle).await;
    f.wt.put(&worker.worktree, "a.rs", "two\n");
    caller("s2", ToolRole::Worker)
}

async fn check(core: &Core) -> Option<ReviewCheck> {
    reviews(core)
        .await
        .into_iter()
        .next()
        .and_then(|i| i.check.map(|c| *c))
}

/// Waits until the item's check is no longer running.
async fn settled(core: &Core, rx: &mut broadcast::Receiver<Event>) -> Option<ReviewCheck> {
    loop {
        let now = check(core).await;
        if now
            .as_ref()
            .is_none_or(|c| c.state != ReviewCheckState::Running)
        {
            return now;
        }
        match Core::next_event(rx).await {
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

async fn log(core: &Core) -> Result<Vec<String>, ProtocolError> {
    let request = json!({"method": "review.check_log", "params": {"id": "r1"}});
    match call(core, request).await? {
        Response::Review(ReviewResponse::CheckLog { lines }) => Ok(lines),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_workers_review_runs_the_check_in_its_worktrees_pod() {
    let f = checked(Some(r#"{"command": ["just", "check"], "timeout": 900}"#)).await;
    let mut rx = f.core.subscribe();
    let me = worker(&f, &mut rx).await;
    f.dev.hold_setup.store(true, Ordering::SeqCst);
    *f.dev.setup_output.lock().unwrap_or_else(|e| e.into_inner()) =
        vec!["ok 12 tests".into(), "\u{1b}[32mdone\u{1b}[0m".into()];
    assert!(f.core.request_review(&me, None, "done").await.is_ok());

    let running = check(&f.core).await.map(|c| (c.state, c.command));
    assert_eq!(
        running,
        Some((ReviewCheckState::Running, "just check".to_owned()))
    );
    // It runs in the background: the pod gets it soon after.
    let setups = || f.dev.setups.lock().map(|s| s.clone()).unwrap_or_default();
    for _ in 0..100 {
        if !setups().is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let setups = setups();
    let [(pod, steps)] = setups.as_slice() else {
        panic!("{setups:?}");
    };
    let worktree = pod.work_dir.clone();
    assert!(worktree.starts_with(WORKTREES), "{worktree:?}");
    let line = &steps[0].line;
    assert_eq!(
        (line.program.clone(), line.args.clone(), line.cwd.clone()),
        ("just".into(), vec!["check".into()], worktree)
    );
    assert_eq!(steps[0].timeout.as_secs(), 900);

    f.dev.finish_setup();
    let done = settled(&f.core, &mut rx).await.map(|c| (c.state, c.detail));
    assert_eq!(done, Some((ReviewCheckState::Passed, None)));
    assert_eq!(
        log(&f.core).await.ok(),
        Some(vec!["ok 12 tests".into(), "\u{1b}[32mdone\u{1b}[0m".into()])
    );
}

#[tokio::test]
async fn a_failed_or_slow_check_says_why() {
    for (end, want) in [
        (
            SetupEnd::Failed {
                step: 0,
                code: Some(2),
                reason: None,
            },
            "exit code 2",
        ),
        (SetupEnd::TimedOut { step: 0 }, "timed out after 600 s"),
        (
            SetupEnd::Failed {
                step: 0,
                code: None,
                reason: Some("no such program: just".into()),
            },
            "could not start: no such program: just",
        ),
    ] {
        let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
        let mut rx = f.core.subscribe();
        let me = worker(&f, &mut rx).await;
        *f.dev.setup_end.lock().unwrap_or_else(|e| e.into_inner()) = Some(end);
        assert!(f.core.request_review(&me, None, "done").await.is_ok());
        let done = settled(&f.core, &mut rx).await;
        let done = done.map(|c| (c.state, c.detail.unwrap_or_default()));
        assert_eq!(done, Some((ReviewCheckState::Failed, want.to_owned())));
    }
}

#[tokio::test]
async fn without_a_check_nothing_runs_and_a_broken_one_says_what_is_wrong() {
    let f = checked(None).await;
    let mut rx = f.core.subscribe();
    let me = worker(&f, &mut rx).await;
    assert!(f.core.request_review(&me, None, "done").await.is_ok());
    assert_eq!(check(&f.core).await, None);
    assert_eq!(code(log(&f.core).await), ErrorCode::NotFound);

    // The file broke since: the item says so, and nothing runs.
    let root = std::path::Path::new(ROOT);
    f.fs.put(
        root,
        ".ovra/check.json",
        r#"{"command": ["x"], "shell": true}"#,
    );
    assert!(f.core.request_review(&me, None, "again").await.is_ok());
    let broken = check(&f.core).await.map(|c| (c.state, c.detail));
    assert_eq!(
        broken,
        Some((
            ReviewCheckState::Failed,
            Some("the check file is broken".into())
        ))
    );
    let why = log(&f.core).await.unwrap_or_default().join("\n");
    assert!(why.contains("unknown field `shell`"), "{why}");
    assert!(f.dev.setups.lock().map(|s| s.is_empty()).unwrap_or(false));
}

#[tokio::test]
async fn the_check_goes_once_the_worker_works_again() {
    let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
    let mut rx = f.core.subscribe();
    let me = worker(&f, &mut rx).await;
    f.dev.hold_setup.store(true, Ordering::SeqCst);
    assert!(f.core.request_review(&me, None, "done").await.is_ok());
    let running = check(&f.core).await.map(|c| c.state);
    assert_eq!(running, Some(ReviewCheckState::Running));

    // A message wakes it: the files the check judged are gone, and the
    // running command with them.
    let boss = caller("s1", ToolRole::Coordinator);
    assert!(
        f.core
            .send_message(&boss, &sid("s2"), "one more thing")
            .is_ok()
    );
    until(&mut rx, "s2", AgentStatus::Idle).await;
    assert_eq!(check(&f.core).await, None);
    // The finish of the killed run changes nothing.
    f.dev.finish_setup();
    assert_eq!(check(&f.core).await, None);
}

/// Waits until the fake pod got `n` runs.
async fn setups(f: &Checked, n: usize) -> usize {
    let count = || f.dev.setups.lock().map(|s| s.len()).unwrap_or(0);
    for _ in 0..100 {
        if count() >= n {
            break;
        }
        tokio::task::yield_now().await;
    }
    count()
}

#[tokio::test]
async fn a_new_request_replaces_the_running_check_and_a_stop_ends_it() {
    let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
    let mut rx = f.core.subscribe();
    let me = worker(&f, &mut rx).await;
    f.dev.hold_setup.store(true, Ordering::SeqCst);
    assert!(f.core.request_review(&me, None, "done").await.is_ok());
    assert_eq!(setups(&f, 1).await, 1);
    assert!(
        f.core
            .request_review(&me, None, "done, really")
            .await
            .is_ok()
    );
    assert_eq!(setups(&f, 2).await, 2);
    let running = check(&f.core).await.map(|c| c.state);
    assert_eq!(running, Some(ReviewCheckState::Running));

    // An unfinished check ends with its worker, and its end changes nothing.
    let boss = caller("s1", ToolRole::Coordinator);
    assert!(f.core.stop_agent(&boss, &sid("s2")).is_ok());
    until(&mut rx, "s2", AgentStatus::Stopped).await;
    assert_eq!(check(&f.core).await, None);
    f.dev.finish_setup();
    assert_eq!(check(&f.core).await, None);
}

#[tokio::test]
async fn a_request_during_the_turn_runs_once_the_turn_ends() {
    let bash = Step::Use {
        tool: "Bash",
        input: json!({"command": "make"}),
    };
    let agent = ScriptedAgent::new(vec![vec![Step::Say("on it")], vec![bash]]);
    let f = checked_with(Some(r#"{"command": ["just", "check"]}"#), agent).await;
    let mut rx = f.core.subscribe();
    let me = worker(&f, &mut rx).await;
    // Its next turn waits for the person, mid-turn, when it asks for review.
    let boss = caller("s1", ToolRole::Coordinator);
    assert!(f.core.send_message(&boss, &sid("s2"), "build it").is_ok());
    let asked = loop {
        match Core::next_event(&mut rx).await {
            Some(Event::Permissions(PermissionsEvent::Requested(r))) => break r.id.clone(),
            Some(_) => {}
            None => panic!("core closed"),
        }
    };
    assert!(f.core.request_review(&me, None, "done").await.is_ok());
    assert_eq!(setups(&f, 1).await, 0, "not during the turn");
    let running = check(&f.core).await.map(|c| c.state);
    assert_eq!(running, Some(ReviewCheckState::Running));

    let answer =
        json!({"method": "permissions.answer", "params": {"id": asked, "decision": "allow"}});
    ok(&f.core, answer).await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    let done = settled(&f.core, &mut rx).await.map(|c| c.state);
    assert_eq!(done, Some(ReviewCheckState::Passed));
    assert_eq!(setups(&f, 1).await, 1);
}
