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

/// Waits until `f`'s fake pod got `n` runs, then lets them finish.
async fn passes(f: &Checked, rx: &mut broadcast::Receiver<Event>, n: usize) -> Option<ReviewCheck> {
    let runs = || f.dev.setups.lock().map(|s| s.len()).unwrap_or(0);
    loop {
        if runs() >= n {
            break;
        }
        match Core::next_event(rx).await {
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
    settled(&f.core, rx).await
}

/// A solo task's agent, `s1`, idle after its first turn; its worktree.
async fn solo(f: &Checked, rx: &mut broadcast::Receiver<Event>) -> String {
    let start = json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "fix it", "agent": "claude", "solo": true}});
    let Response::Orchestration(OrchestrationResponse::Task(task)) = ok(&f.core, start).await
    else {
        panic!("no task");
    };
    until(rx, "s1", AgentStatus::Idle).await;
    task.workers[0].worktree.clone()
}

async fn turn(core: &Core, rx: &mut broadcast::Receiver<Event>, text: &str) {
    let send = json!({"method": "agents.send", "params": {"session": "s1", "text": text}});
    ok(core, send).await;
    until(rx, "s1", AgentStatus::Working).await;
    until(rx, "s1", AgentStatus::Idle).await;
}

#[tokio::test]
async fn a_persons_own_agent_is_checked_when_a_turn_leaves_new_changes() {
    let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
    let mut rx = f.core.subscribe();
    let worktree = solo(&f, &mut rx).await;
    // A turn without changes runs nothing.
    assert_eq!(check(&f.core).await, None);

    f.wt.put(&worktree, "a.rs", "two\n");
    turn(&f.core, &mut rx, "go on").await;
    let done = passes(&f, &mut rx, 1).await.map(|c| (c.state, c.stale));
    assert_eq!(done, Some((ReviewCheckState::Passed, false)));
    let pods = f.dev.setups.lock().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(pods[0].0.work_dir, PathBuf::from(&worktree));

    // A turn that changes nothing keeps the result; the files it judged
    // are still there.
    turn(&f.core, &mut rx, "anything else?").await;
    let kept = check(&f.core).await.map(|c| (c.state, c.stale));
    assert_eq!(kept, Some((ReviewCheckState::Passed, false)));
    assert_eq!(f.dev.setups.lock().map(|s| s.len()).unwrap_or(0), 1);

    // Files changed outside a turn: the result is stale until it runs again.
    f.wt.put(&worktree, "a.rs", "three\n");
    let stale = check(&f.core).await.map(|c| (c.state, c.stale));
    assert_eq!(stale, Some((ReviewCheckState::Passed, true)));
    *f.dev.setup_end.lock().unwrap_or_else(|e| e.into_inner()) = Some(SetupEnd::Failed {
        step: 0,
        code: Some(1),
        reason: None,
    });
    ok(
        &f.core,
        json!({"method": "review.run_check", "params": {"id": "r1"}}),
    )
    .await;
    let again = passes(&f, &mut rx, 2).await.map(|c| (c.state, c.stale));
    assert_eq!(again, Some((ReviewCheckState::Failed, false)));
}

#[tokio::test]
async fn a_finished_check_is_kept_across_a_restart_without_its_log() {
    let projects = Arc::new(MemoryProjects::default());
    let sessions = Arc::new(crate::fakes::MemorySessions::default());
    let wt = Arc::new(MemoryWorktrees::default());
    let fs = Arc::new(MemoryFs::default());
    let dev = Arc::new(FakeDevServers::default());
    wt.put(ROOT, "a.rs", "one\n");
    fs.put(
        std::path::Path::new(ROOT),
        ".ovra/check.json",
        r#"{"command": ["just", "check"]}"#,
    );
    let build = || {
        Core::builder("test")
            .projects(projects.clone())
            .sessions(sessions.clone())
            .fs(fs.clone())
            .agents(Arc::new(ScriptedAgent::new(say())))
            .worktrees(wt.clone())
            .worktrees_dir(PathBuf::from(WORKTREES))
            .dev_servers(dev.clone())
            .build()
    };
    let f = Checked {
        core: build(),
        dev: dev.clone(),
        fs: fs.clone(),
        wt: wt.clone(),
    };
    ok(
        &f.core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let mut rx = f.core.subscribe();
    let worktree = solo(&f, &mut rx).await;
    f.wt.put(&worktree, "a.rs", "two\n");
    turn(&f.core, &mut rx, "go on").await;
    assert!(passes(&f, &mut rx, 1).await.is_some());
    drop(f);

    let after = build();
    let kept = check(&after).await.map(|c| (c.state, c.stale));
    assert_eq!(kept, Some((ReviewCheckState::Passed, false)));
    let lines = log(&after).await.unwrap_or_default();
    assert_eq!(
        lines,
        ["Ovra keeps a check's output only until it restarts."]
    );
}

#[tokio::test]
async fn a_direct_agent_is_checked_in_the_main_checkouts_pod() {
    let runtime = Arc::new(WithDirect {
        scripted: ScriptedAgent::new(say()),
        direct: std::sync::Mutex::new(None),
    });
    let wt = Arc::new(MemoryWorktrees::default());
    let fs = Arc::new(MemoryFs::default());
    let dev = Arc::new(FakeDevServers::default());
    wt.put(ROOT, "a.rs", "one\n");
    fs.put(
        std::path::Path::new(ROOT),
        ".ovra/check.json",
        r#"{"command": ["just", "check"]}"#,
    );
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(runtime.clone())
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .dev_servers(dev.clone())
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
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "go"}}),
    )
    .await;
    let fx = DirectFx {
        core: core.clone(),
        wt: wt.clone(),
        runtime: runtime.clone(),
        worktree: String::new(),
    };
    let host = runtime
        .direct
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| panic!("no direct session"));
    host.status(AgentStatus::Working);
    fx.direct_writes("a.rs", "two\n").await;
    host.status(AgentStatus::Idle);
    let f = Checked { core, dev, fs, wt };
    let done = passes(&f, &mut rx, 1).await.map(|c| c.state);
    assert_eq!(done, Some(ReviewCheckState::Passed));
    let pods = f.dev.setups.lock().map(|s| s.clone()).unwrap_or_default();
    let (pod, steps) = &pods[0];
    assert_eq!(pod.work_dir, PathBuf::from(ROOT));
    assert_eq!(steps[0].line.cwd, PathBuf::from(ROOT));
    // The main checkout's own pod: a worker's would also read `<root>/.git`.
    assert!(pod.profile.read.is_empty(), "{:?}", pod.profile.read);
}

#[tokio::test]
async fn warnings_point_at_the_tests_an_agent_weakened() {
    let f = checked(None).await;
    let mut rx = f.core.subscribe();
    f.wt.put(
        ROOT,
        "src/a.test.ts",
        "it('adds', () => {\n  expect(add(1, 1)).toBe(2);\n});\n",
    );
    let worktree = solo(&f, &mut rx).await;
    f.wt.put(&worktree, "src/a.test.ts", "it.skip('adds', () => {\n});\n");
    f.wt.put(
        &worktree,
        "src/b.ts",
        "// eslint-disable-next-line\nexport const b = 1;\n",
    );
    let request = json!({"method": "review.warnings", "params": {"id": "r1"}});
    let Response::Review(ReviewResponse::Warnings { warnings }) = ok(&f.core, request).await else {
        panic!("no warnings");
    };
    let found: Vec<_> = warnings
        .iter()
        .map(|w| (w.kind, w.path.as_str(), w.line, w.old))
        .collect();
    use crate::protocol::review::ReviewWarningKind as K;
    assert_eq!(
        found,
        [
            (K::AssertionRemoved, "src/a.test.ts", Some(2), true),
            (K::TestSkipped, "src/a.test.ts", Some(1), false),
            (K::CheckSilenced, "src/b.ts", Some(1), false),
        ]
    );
}

#[tokio::test]
async fn a_coordinators_worker_is_checked_only_when_review_is_asked() {
    let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
    let mut rx = f.core.subscribe();
    let _me = worker(&f, &mut rx).await;
    let boss = caller("s1", ToolRole::Coordinator);
    assert!(f.core.send_message(&boss, &sid("s2"), "go on").is_ok());
    until(&mut rx, "s2", AgentStatus::Working).await;
    until(&mut rx, "s2", AgentStatus::Idle).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(f.dev.setups.lock().map(|s| s.len()).unwrap_or(0), 0);
    assert_eq!(check(&f.core).await, None);
}

#[tokio::test]
async fn run_again_works_for_an_ended_agent_and_not_for_a_decided_item() {
    let f = checked(Some(r#"{"command": ["just", "check"]}"#)).await;
    let mut rx = f.core.subscribe();
    let worktree = solo(&f, &mut rx).await;
    f.wt.put(&worktree, "a.rs", "two\n");
    let stop = json!({"method": "agents.stop", "params": {"session": "s1"}});
    ok(&f.core, stop).await;
    until(&mut rx, "s1", AgentStatus::Stopped).await;
    let run = json!({"method": "review.run_check", "params": {"id": "r1"}});
    ok(&f.core, run.clone()).await;
    let done = passes(&f, &mut rx, 1).await.map(|c| c.state);
    assert_eq!(done, Some(ReviewCheckState::Passed));

    let reject = json!({"method": "review.reject", "params": {"id": "r1"}});
    ok(&f.core, reject).await;
    assert_eq!(code(call(&f.core, run).await), ErrorCode::Conflict);
}
