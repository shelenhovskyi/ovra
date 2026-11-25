use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::json;
use tokio::sync::broadcast;

use crate::Core;
use crate::fakes::{
    FakeDevServers, MemoryFs, MemoryProjects, MemoryWorktrees, ScriptedAgent, Step,
};
use crate::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, MessageFrom, SessionId};
use crate::protocol::orchestration::{OrchestrationResponse, Task, Worker};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::{Included, IncludedFile, SetupEnd, ToolCaller, ToolRole};

const ROOT: &str = crate::test_path!("/work/a");
const WORKTREES: &str = crate::test_path!("/data/worktrees");
const SETUP: &str = r#"{"setup": [{"command": ["pnpm", "install"]}, {"command": ["sh", ".ovra/setup.sh"], "timeout": 900}]}"#;

struct Fx {
    core: Core,
    agent: Arc<ScriptedAgent>,
    wt: Arc<MemoryWorktrees>,
    fs: Arc<MemoryFs>,
    dev: Arc<FakeDevServers>,
}

async fn fx() -> Fx {
    let agent = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("on it")]]));
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let fs = Arc::new(MemoryFs::default());
    let dev = Arc::new(FakeDevServers::default());
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(agent.clone())
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .dev_servers(dev.clone())
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    ok(
        &core,
        json!({"method": "orchestration.start", "params": {
            "project": "p1", "prompt": "split the work", "limits": {"max_workers": 3}}}),
    )
    .await;
    Fx {
        core,
        agent,
        wt,
        fs,
        dev,
    }
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response, ProtocolError> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json).await.unwrap_or_else(|e| panic!("{e}"))
}

fn boss() -> ToolCaller {
    ToolCaller {
        session: SessionId("s1".into()),
        project: ProjectId("p1".into()),
        role: ToolRole::Coordinator,
    }
}

fn dir(name: &str) -> PathBuf {
    Path::new(WORKTREES).join("p1").join("t1").join(name)
}

/// Puts `.ovra/worktree.json` with `text` where worker `name`'s worktree goes.
fn setup_file(f: &Fx, name: &str, text: &str) {
    f.fs.put(&dir(name), ".ovra/worktree.json", text);
}

async fn task(core: &Core) -> Task {
    let list = ok(
        core,
        json!({"method": "orchestration.list", "params": {"project": "p1"}}),
    )
    .await;
    match list {
        Response::Orchestration(OrchestrationResponse::List(mut tasks)) => tasks.remove(0),
        other => panic!("unexpected {other:?}"),
    }
}

async fn worker(core: &Core, name: &str) -> Worker {
    let task = task(core).await;
    let found = task.workers.into_iter().find(|w| w.name == name);
    found.unwrap_or_else(|| panic!("no worker {name}"))
}

fn status(core: &Core, session: &SessionId) -> AgentStatus {
    core.sessions()
        .find(session, |s| s.status().clone())
        .unwrap_or_else(|e| panic!("{e}"))
}

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

fn started(f: &Fx) -> usize {
    f.agent.started.lock().map(|s| s.len()).unwrap_or_default()
}

fn working(core: &Core) -> u32 {
    super::slots::capacity(core, &ProjectId("p1".into())).working
}

#[tokio::test]
async fn a_worker_is_prepared_in_its_locked_worktree_then_starts() {
    let f = fx().await;
    setup_file(&f, "w", SETUP);
    f.dev.hold_setup.store(true, Ordering::SeqCst);
    let mut rx = f.core.subscribe();
    let core = f.core.clone();
    let spawn = tokio::spawn(async move { core.spawn_agent(&boss(), "w", "do it", false).await });
    until(&mut rx, "s2", AgentStatus::Preparing).await;
    tokio::task::yield_now().await;
    // Preparing takes a place, and the agent waits.
    assert_eq!(working(&f.core), 1);
    assert_eq!(started(&f), 1);
    let locked = f.wt.locked.lock().map(|l| l.clone()).unwrap_or_default();
    assert_eq!(
        locked.get(&dir("w")).map(String::as_str),
        Some("Ovra: t1/w")
    );

    let setups = f.dev.setups.lock().map(|s| s.clone()).unwrap_or_default();
    let (pod, steps) = &setups[0];
    assert_eq!(pod.work_dir, dir("w"));
    let line = &steps[1].line;
    assert_eq!(line.program, PathBuf::from("sh"));
    assert_eq!(line.cwd, dir("w"));
    assert_eq!(steps[1].timeout.as_secs(), 900);
    let env: Vec<(String, String)> = line
        .env
        .iter()
        .map(|(k, v)| {
            let v = v.as_ref().map(|v| v.to_string_lossy().into_owned());
            (k.to_string_lossy().into_owned(), v.unwrap_or_default())
        })
        .collect();
    let want = [
        (
            "OVRA_WORKTREE_ROOT",
            dir("w").to_string_lossy().into_owned(),
        ),
        ("OVRA_MAIN_ROOT", ROOT.to_owned()),
        ("OVRA_PORT", "41000".to_owned()),
    ];
    let want: Vec<(String, String)> = want.into_iter().map(|(k, v)| (k.into(), v)).collect();
    assert_eq!(env, want);
    let shown = worker(&f.core, "w")
        .await
        .setup
        .map(|s| (s.step, s.steps, s.command));
    assert_eq!(shown, Some((1, 2, "sh .ovra/setup.sh".to_owned())));

    f.dev.finish_setup();
    let spawned = spawn.await.ok().and_then(Result::ok);
    assert_eq!(spawned.map(|w| w.setup), Some(None));
    assert_eq!(started(&f), 2);
    // Counted once it started.
    let mut disk = None;
    while disk.is_none() {
        tokio::task::yield_now().await;
        disk = worker(&f.core, "w").await.disk;
    }
}

#[tokio::test]
async fn a_failed_step_keeps_the_worker_from_starting_until_it_is_skipped() {
    let f = fx().await;
    setup_file(&f, "w", SETUP);
    *f.dev.setup_end.lock().unwrap_or_else(|e| e.into_inner()) = Some(SetupEnd::Failed {
        step: 0,
        code: Some(1),
        reason: None,
    });
    let output: Vec<String> = (1..=60).map(|n| format!("line {n}")).collect();
    *f.dev.setup_output.lock().unwrap_or_else(|e| e.into_inner()) = output;
    let err = f.core.spawn_agent(&boss(), "w", "do it", false).await;
    let err = err.err().unwrap_or_else(|| panic!("it started"));
    assert_eq!(err.code, ErrorCode::Conflict);
    for part in [
        "worker w (agent_id s2) did not start",
        "step 1 of 2 (pnpm install) exited with code 1",
        "<untrusted origin=\"setup\">line 11\n",
        "line 60</untrusted>",
        "skip_setup",
    ] {
        assert!(err.message.contains(part), "{part}: {}", err.message);
    }
    assert!(!err.message.contains("line 10\n"));
    let failed = worker(&f.core, "w").await.setup;
    assert_eq!(failed.as_ref().map(|s| s.log.len()), Some(50));
    assert_eq!(
        failed.and_then(|s| s.failed).as_deref(),
        Some("exited with code 1")
    );
    let session = SessionId("s2".into());
    assert_eq!(status(&f.core, &session), AgentStatus::Preparing);
    // It waits for a decision and takes no place meanwhile.
    assert_eq!(working(&f.core), 0);
    assert_eq!(started(&f), 1);

    let worker = f.core.spawn_agent(&boss(), "w", "ignored", true).await;
    assert_eq!(
        worker.ok().map(|w| (w.session.0, w.setup)),
        Some(("s2".into(), None))
    );
    assert_eq!(started(&f), 2);
    assert_eq!(f.dev.setups.lock().map(|s| s.len()).unwrap_or_default(), 1);
}

#[tokio::test]
async fn the_person_retries_a_failed_setup() {
    let f = fx().await;
    setup_file(&f, "w", SETUP);
    *f.dev.setup_end.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(SetupEnd::TimedOut { step: 1 });
    let err = f.core.spawn_agent(&boss(), "w", "do it", false).await;
    let message = err.err().map(|e| e.message).unwrap_or_default();
    assert!(
        message.contains("step 2 of 2 (sh .ovra/setup.sh) ran longer than its timeout"),
        "{message}"
    );
    let retry = json!({"method": "orchestration.retry_setup", "params": {"session": "s2"}});
    let mut rx = f.core.subscribe();
    ok(&f.core, retry.clone()).await;
    until(&mut rx, "s2", AgentStatus::Starting).await;
    assert_eq!(f.dev.setups.lock().map(|s| s.len()).unwrap_or_default(), 2);
    // Only a failed setup can be retried.
    let again = call(&f.core, retry).await.err().map(|e| e.code);
    assert_eq!(again, Some(ErrorCode::Conflict));
}

#[tokio::test]
async fn a_broken_file_starts_the_worker_without_setup_and_says_so() {
    let f = fx().await;
    setup_file(&f, "w", r#"{"setup": [{"command": []}]}"#);
    let worker = f.core.spawn_agent(&boss(), "w", "do it", false).await;
    assert!(worker.is_ok());
    assert!(
        f.dev
            .setups
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or_default()
    );
    let transcript = f
        .core
        .sessions()
        .find(&SessionId("s2".into()), |s| s.transcript())
        .unwrap_or_default();
    let Some(AgentMessage::UserText { text, from }) = transcript.first() else {
        panic!("no first message: {transcript:?}");
    };
    assert_eq!(*from, MessageFrom::Ovra);
    assert!(
        text.contains("was not set up") && text.contains("the command is empty"),
        "{text}"
    );
}

#[tokio::test]
async fn a_stop_while_preparing_ends_the_setup() {
    let f = fx().await;
    setup_file(&f, "w", SETUP);
    f.dev.hold_setup.store(true, Ordering::SeqCst);
    let mut rx = f.core.subscribe();
    let core = f.core.clone();
    let spawn = tokio::spawn(async move { core.spawn_agent(&boss(), "w", "do it", false).await });
    until(&mut rx, "s2", AgentStatus::Preparing).await;
    ok(
        &f.core,
        json!({"method": "agents.stop", "params": {"session": "s2"}}),
    )
    .await;
    let result = spawn.await.ok().map(|r| r.err().map(|e| e.code));
    assert_eq!(result, Some(Some(ErrorCode::Conflict)));
    assert_eq!(started(&f), 1);
    assert_eq!(working(&f.core), 0);
}

#[tokio::test]
async fn workers_get_ports_of_their_own_and_their_copies_go_with_them() {
    let f = fx().await;
    let copied = IncludedFile {
        path: ".env".into(),
        bytes: 4,
        hash: "h".into(),
    };
    f.wt.put(ROOT, ".env", "A=1\n");
    *f.wt.to_include.lock().unwrap_or_else(|e| e.into_inner()) = Included {
        copied: vec![copied.clone()],
        skipped: Vec::new(),
    };
    for name in ["a", "b"] {
        setup_file(&f, name, r#"{"setup": [{"command": ["true"]}]}"#);
        assert!(
            f.core
                .spawn_agent(&boss(), name, "do it", false)
                .await
                .is_ok()
        );
    }
    let port = |n: usize| {
        let setups = f.dev.setups.lock().map(|s| s.clone()).unwrap_or_default();
        let env = setups[n].1[0].line.env.clone();
        env.into_iter()
            .find(|(k, _)| k == "OVRA_PORT")
            .and_then(|(_, v)| v)
            .map(|v| v.to_string_lossy().into_owned())
    };
    assert_eq!(
        (port(0).as_deref(), port(1).as_deref()),
        (Some("41000"), Some("41010"))
    );
    assert_eq!(
        f.wt.files(dir("a")).get(".env").map(String::as_str),
        Some("A=1\n")
    );

    // The task ends with nothing left to review: both worktrees go.
    for session in ["s2", "s3"] {
        ok(
            &f.core,
            json!({"method": "agents.stop", "params": {"session": session}}),
        )
        .await;
    }
    ok(
        &f.core,
        json!({"method": "orchestration.stop", "params": {"task": "t1"}}),
    )
    .await;
    let mut removed = Vec::new();
    while removed.len() < 2 {
        tokio::task::yield_now().await;
        task(&f.core).await;
        removed = f.wt.removed.lock().map(|r| r.clone()).unwrap_or_default();
    }
    assert_eq!(removed[0], (dir("a"), vec![copied]));
    assert!(f.wt.locked.lock().map(|l| l.is_empty()).unwrap_or_default());
    // Their ports are free again.
    let state = f.core.orchestration_state().lock();
    assert_eq!(super::free_port(&state), Some(41000));
}

#[tokio::test(start_paused = true)]
async fn a_new_worktree_waits_while_the_disk_is_low() {
    let f = fx().await;
    *f.wt.free.lock().unwrap_or_else(|e| e.into_inner()) = Some(1024 * 1024 * 1024);
    let worker = f.core.spawn_agent(&boss(), "w", "do it", false).await;
    let waiting = worker.ok().and_then(|w| w.waiting).unwrap_or_default();
    assert!(
        waiting.contains("1.0 GiB free") && waiting.contains("5 GiB"),
        "{waiting}"
    );
    let session = SessionId("s2".into());
    assert_eq!(status(&f.core, &session), AgentStatus::Queued);
    assert!(!f.wt.has(dir("w")));

    // Asked again at most once a minute, when places are handed out.
    *f.wt.free.lock().unwrap_or_else(|e| e.into_inner()) = Some(100 * 1024 * 1024 * 1024);
    let mut rx = f.core.subscribe();
    super::slots::start_queued(&f.core, &ProjectId("p1".into()));
    tokio::task::yield_now().await;
    assert_eq!(status(&f.core, &session), AgentStatus::Queued);
    tokio::time::advance(super::DISK_RECHECK).await;
    super::slots::start_queued(&f.core, &ProjectId("p1".into()));
    until(&mut rx, "s2", AgentStatus::Starting).await;
    assert!(f.wt.has(dir("w")));
    assert_eq!(worker_waiting(&f.core).await, None);
}

async fn worker_waiting(core: &Core) -> Option<String> {
    worker(core, "w").await.waiting
}

#[tokio::test(start_paused = true)]
async fn disk_use_is_counted_at_most_once_a_minute() {
    let f = fx().await;
    assert!(
        f.core
            .spawn_agent(&boss(), "w", "do it", false)
            .await
            .is_ok()
    );
    let session = SessionId("s2".into());
    let mut disk = None;
    while disk.is_none() {
        tokio::task::yield_now().await;
        disk = worker(&f.core, "w").await.disk;
    }
    let first = disk.map(|d| d.bytes).unwrap_or_default();
    f.wt.put(dir("w"), "big.bin", "0123456789");
    super::count_disk(&f.core, &session);
    tokio::task::yield_now().await;
    assert_eq!(
        worker(&f.core, "w").await.disk.map(|d| d.bytes),
        Some(first)
    );
    tokio::time::advance(super::USAGE_EVERY).await;
    super::count_disk(&f.core, &session);
    let mut now = first;
    while now == first {
        tokio::task::yield_now().await;
        now = worker(&f.core, "w")
            .await
            .disk
            .map(|d| d.bytes)
            .unwrap_or_default();
    }
    assert_eq!(now, first + 10);
}
