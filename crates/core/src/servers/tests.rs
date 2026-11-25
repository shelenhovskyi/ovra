use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::Core;
use crate::fakes::{
    FakeDevServers, FakeNetProxy, MemoryAudit, MemoryFs, MemoryProjects, MemoryWorktrees,
    ScriptedAgent, Step,
};
use crate::protocol::agents::{AgentStatus, AgentsEvent, AgentsResponse, SessionId};
use crate::protocol::audit::{AuditEvent, AuditRange};
use crate::protocol::projects::ProjectId;
use crate::protocol::servers::{
    ServerInfo, ServerList, ServerStatus, ServersEvent, ServersResponse,
};
use crate::protocol::{ErrorCode, Event, ProtocolError, Request, Response};
use crate::services::{AuditLog, ToolCaller, ToolRole};

const ROOT: &str = crate::test_path!("/work/a");
const WORKTREES: &str = crate::test_path!("/data/worktrees");
const SERVERS: &str = r#"{"servers": [
    {"name": "web", "command": ["pnpm", "dev"], "cwd": "ui", "port": 5173},
    {"name": "api", "command": ["cargo", "run"], "port": 8080}
]}"#;

struct Fx {
    core: Core,
    dev: Arc<FakeDevServers>,
    net: Arc<FakeNetProxy>,
    fs: Arc<MemoryFs>,
    wt: Arc<MemoryWorktrees>,
    audit: Arc<MemoryAudit>,
    agent: Arc<ScriptedAgent>,
}

/// Project `p1` at [`ROOT`] defining `web` and `api`, every service fake.
async fn fx() -> Fx {
    let fs = Arc::new(MemoryFs::default());
    fs.put(Path::new(ROOT), ".ovra/servers.json", SERVERS);
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let (dev, net) = (
        Arc::new(FakeDevServers::default()),
        Arc::new(FakeNetProxy::default()),
    );
    let audit = Arc::new(MemoryAudit::default());
    let agent = Arc::new(ScriptedAgent::new(vec![vec![Step::Say("on it")]]));
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs.clone())
        .agents(agent.clone())
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .netproxy(net.clone())
        .dev_servers(dev.clone())
        .audit(audit.clone())
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    Fx {
        core,
        dev,
        net,
        fs,
        wt,
        audit,
        agent,
    }
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

/// A coordinator `s1` in the main checkout and its worker `s2` in a
/// worktree that defines only `web`. Returns the worktree.
async fn coordinator_and_worker(f: &Fx) -> PathBuf {
    ok(&f.core, json!({"method": "orchestration.start", "params": {
        "project": "p1", "prompt": "split", "limits": {"max_workers": 1, "budget_micro_usd": null}}}))
    .await;
    let boss = caller("s1", ToolRole::Coordinator);
    let worker = f.core.spawn_agent(&boss, "w", "fix", false).await;
    let worktree = PathBuf::from(worker.unwrap_or_else(|e| panic!("{e}")).worktree);
    let only_web = r#"{"servers": [{"name": "web", "command": ["pnpm", "dev"], "port": 5173}]}"#;
    f.fs.put(&worktree, ".ovra/servers.json", only_web);
    worktree
}

async fn person_starts(f: &Fx, name: &str) -> ServerInfo {
    let start = json!({"method": "servers.start", "params": {"project": "p1", "name": name}});
    match ok(&f.core, start).await {
        Response::Servers(ServersResponse::Server(info)) => info,
        other => panic!("unexpected {other:?}"),
    }
}

async fn starts_in(
    f: &Fx,
    name: &str,
    work_dir: Option<&str>,
) -> Result<ServerInfo, ProtocolError> {
    let start = json!({"method": "servers.start", "params": {"project": "p1", "name": name, "work_dir": work_dir}});
    match call(&f.core, start).await? {
        Response::Servers(ServersResponse::Server(info)) => Ok(info),
        other => panic!("unexpected {other:?}"),
    }
}

async fn list(f: &Fx, work_dir: Option<&str>) -> ServerList {
    let list = json!({"method": "servers.list", "params": {"project": "p1", "work_dir": work_dir}});
    match ok(&f.core, list).await {
        Response::Servers(ServersResponse::List(list)) => list,
        other => panic!("unexpected {other:?}"),
    }
}

/// Each session's non-empty grants: port and generation.
fn grants(f: &Fx) -> BTreeMap<String, Vec<(u16, u64)>> {
    let grants = f.net.grants.lock().map(|g| g.clone()).unwrap_or_default();
    grants
        .into_iter()
        .filter(|(_, g)| !g.is_empty())
        .map(|(s, g)| (s.0, g))
        .collect()
}

fn proxies(f: &Fx) -> Vec<String> {
    let running = f
        .net
        .running
        .lock()
        .map(|r| r.keys().cloned().collect::<Vec<_>>());
    running
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.0)
        .collect()
}

/// The audit entries about servers and grants, as `session: event`.
fn server_audit(f: &Fx) -> Vec<String> {
    let entries = f
        .audit
        .list(&ProjectId("p1".into()), AuditRange::default(), 1000)
        .unwrap_or_default();
    entries
        .into_iter()
        .filter(|e| {
            matches!(
                e.event,
                AuditEvent::ServerStarted { .. }
                    | AuditEvent::ServerStopped { .. }
                    | AuditEvent::LoopbackGranted { .. }
                    | AuditEvent::LoopbackRevoked { .. }
            )
        })
        .map(|e| {
            let session = e.session.map_or_else(|| "-".to_owned(), |s| s.0);
            format!(
                "{session}: {}",
                serde_json::to_string(&e.event).unwrap_or_default()
            )
        })
        .collect()
}

#[tokio::test]
async fn list_shows_each_work_dirs_own_file_and_why_it_is_broken() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    // A direct session's work dir is the main checkout; a worker's its worktree.
    assert_eq!(list(&f, Some("s1")).await, list(&f, None).await);
    let worker = list(&f, Some("s2")).await;
    let broken = r#"{"servers": [{"name": "web"}]}"#;
    f.fs.put(Path::new(ROOT), ".ovra/servers.json", broken);
    let broken = list(&f, None).await;
    let start = json!({"method": "servers.start", "params": {"project": "p1", "name": "web"}});
    assert_eq!(code(call(&f.core, start).await), ErrorCode::BadRequest);
    insta::assert_debug_snapshot!((worker, broken));
}

#[tokio::test]
async fn a_ready_server_is_granted_to_the_sessions_of_its_work_dir_only() {
    let f = fx().await;
    let worktree = coordinator_and_worker(&f).await;
    // Sessions forward the ports their work dir defines.
    let forwards: Vec<Vec<u16>> = f
        .agent
        .started
        .lock()
        .map(|s| s.iter().map(|spec| spec.sandbox.forward.clone()).collect())
        .unwrap_or_default();
    assert_eq!(
        forwards,
        [vec![5173, 8080], vec![]],
        "the worker started before its file"
    );

    let web = person_starts(&f, "web").await;
    assert_eq!(
        (web.id.0.as_str(), &web.status, web.generation),
        ("srv1", &ServerStatus::Starting, 1)
    );
    assert!(grants(&f).is_empty(), "not ready yet");
    assert!(f.dev.report(&web.id, ServerStatus::Ready));
    assert_eq!(
        grants(&f),
        BTreeMap::from([("s1".to_owned(), vec![(5173, 1)])])
    );

    let own = starts_in(&f, "web", Some("s2")).await;
    let own = own.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(own.work_dir, Some(SessionId("s2".into())));
    // The worktree defines no api; the main checkout's web already runs.
    assert_eq!(
        code(starts_in(&f, "api", Some("s2")).await),
        ErrorCode::NotFound
    );
    assert_eq!(code(starts_in(&f, "web", None).await), ErrorCode::Conflict);
    assert!(f.dev.report(&own.id, ServerStatus::Ready));
    assert_eq!(
        grants(&f),
        BTreeMap::from([
            ("s1".to_owned(), vec![(5173, 1)]),
            ("s2".to_owned(), vec![(5173, 1)]),
        ])
    );
    // Each pod has its own work dir, profile and netproxy.
    let pods: Vec<(PathBuf, Vec<u16>, String)> = f
        .dev
        .running
        .lock()
        .map(|r| {
            r.values()
                .map(|s| {
                    (
                        s.pod.work_dir.clone(),
                        s.pod.profile.expose.clone(),
                        format!("{:?} {:?}", s.pod.profile.network, s.pod.profile.kind),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(pods[0].0, PathBuf::from(ROOT));
    assert_eq!(pods[0].1, [5173, 8080]);
    assert!(pods[0].2.contains("fake-proxy:pod-1"), "{}", pods[0].2);
    assert!(
        pods[0].2.ends_with(" Pod"),
        "a pod gets none of claude's state"
    );
    assert_eq!(pods[1].0, worktree);
    assert!(pods[1].2.contains("fake-proxy:pod-2"), "{}", pods[1].2);
    let line = f.dev.running.lock().map(|r| r[&web.id].spec.line.clone());
    assert_eq!(
        line.map(|l| l.cwd).ok(),
        Some(PathBuf::from(ROOT).join("ui"))
    );
}

#[tokio::test]
async fn restart_bumps_the_generation_and_regrants() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let web = person_starts(&f, "web").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    let restart = json!({"method": "servers.restart", "params": {"id": "srv1"}});
    let again = match ok(&f.core, restart).await {
        Response::Servers(ServersResponse::Server(info)) => info,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(
        (again.id, again.generation, again.status),
        (web.id.clone(), 2, ServerStatus::Starting)
    );
    assert!(grants(&f).is_empty(), "the old generation's grant is gone");
    // A late report of the old generation changes nothing.
    let sink = f.dev.running.lock().map(|r| r[&web.id].sink.clone());
    let sink = sink.unwrap_or_else(|e| panic!("{e}"));
    sink.status(&web.id, 1, ServerStatus::Ready);
    assert!(grants(&f).is_empty());
    f.dev.report(&web.id, ServerStatus::Ready);
    assert_eq!(
        grants(&f),
        BTreeMap::from([("s1".to_owned(), vec![(5173, 2)])])
    );
    insta::assert_json_snapshot!(server_audit(&f));
}

#[tokio::test]
async fn an_exit_revokes_and_ends_the_pod_with_its_last_server() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let web = person_starts(&f, "web").await;
    let api = person_starts(&f, "api").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    f.dev.report(&api.id, ServerStatus::Ready);
    assert_eq!(grants(&f)["s1"], [(5173, 1), (8080, 1)]);
    f.dev
        .report(&web.id, ServerStatus::Exited { code: Some(1) });
    assert_eq!(grants(&f)["s1"], [(8080, 1)]);
    assert!(proxies(&f).contains(&"pod-1".to_owned()), "api still runs");
    let stop = json!({"method": "servers.stop", "params": {"id": api.id.0, "grace_ms": 100}});
    ok(&f.core, stop).await;
    let stopped = f.dev.stopped.lock().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(stopped, [(api.id.clone(), Duration::from_millis(100))]);
    assert!(grants(&f).is_empty());
    assert!(!proxies(&f).contains(&"pod-1".to_owned()));
    // A failed start ends its fresh pod too.
    f.fs.put(Path::new(ROOT), ".ovra/servers.json", SERVERS);
    let mut failing = FakeDevServers::default();
    failing.fails = true;
    let failing = Arc::new(failing);
    let core = Core::builder("t")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(f.fs.clone())
        .dev_servers(failing)
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let failed = ok(
        &core,
        json!({"method": "servers.start", "params": {"project": "p1", "name": "web"}}),
    )
    .await;
    assert!(
        matches!(
            failed,
            Response::Servers(ServersResponse::Server(ServerInfo {
                status: ServerStatus::Failed { .. },
                ..
            }))
        ),
        "{failed:?}"
    );
}

#[tokio::test]
async fn a_session_that_ends_loses_its_grants() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let web = person_starts(&f, "web").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    let mut rx = f.core.subscribe();
    ok(
        &f.core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    loop {
        match Core::next_event(&mut rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session,
                status: AgentStatus::Stopped,
                ..
            })) if session.0 == "s1" => break,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
    assert!(grants(&f).is_empty());
    insta::assert_json_snapshot!(server_audit(&f));
}

#[tokio::test]
async fn logs_page_and_grep_and_keep_escapes_for_the_ui() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let web = person_starts(&f, "web").await;
    f.dev.output(
        &web.id,
        &[
            "\u{1b}[1mstarting\u{1b}[0m",
            "error: one",
            "ok",
            "error: two",
        ],
    );
    let logs = json!({"method": "servers.logs", "params": {"id": "srv1", "since": 1, "limit": 2}});
    let Response::Servers(ServersResponse::Logs(page)) = ok(&f.core, logs).await else {
        panic!("not logs");
    };
    assert_eq!((page.lines.len(), page.lines[0].n, page.next), (2, 2, 3));
    let grep = json!({"method": "servers.logs", "params": {"id": "srv1", "grep": "start"}});
    let Response::Servers(ServersResponse::Logs(page)) = ok(&f.core, grep).await else {
        panic!("not logs");
    };
    assert_eq!(page.lines[0].text, "\u{1b}[1mstarting\u{1b}[0m");
    let open = call(
        &f.core,
        json!({"method": "servers.open", "params": {"id": "srv1"}}),
    )
    .await;
    assert_eq!(code(open), ErrorCode::Unavailable);
}

#[tokio::test(start_paused = true)]
async fn output_events_are_batched_at_ten_a_second() {
    let f = fx().await;
    let web = person_starts(&f, "web").await;
    let mut rx = f.core.subscribe();
    for n in 0..5 {
        f.dev.output(&web.id, &[&format!("line {n}")]);
    }
    let mut batches = Vec::new();
    while batches.len() < 2 {
        if let Some(Event::Servers(ServersEvent::Output { lines, .. })) =
            Core::next_event(&mut rx).await
        {
            batches.push(lines.iter().map(|l| l.n).collect::<Vec<_>>());
        }
    }
    // The first line goes at once, the rest after 100 ms in one event.
    assert_eq!(batches, [vec![1], vec![2, 3, 4, 5]]);
    // An end sends what waits before its status.
    f.dev.output(&web.id, &["last"]);
    f.dev
        .report(&web.id, ServerStatus::Exited { code: Some(0) });
    let mut next = Vec::new();
    while next.len() < 2 {
        match Core::next_event(&mut rx).await {
            Some(Event::Servers(ServersEvent::Output { lines, .. })) => {
                next.push(format!("output {}", lines[0].n))
            }
            Some(Event::Servers(ServersEvent::Status { status, .. })) => {
                next.push(format!("{status:?}"))
            }
            _ => {}
        }
    }
    assert_eq!(next, ["output 6", "Exited { code: Some(0) }"]);
}

fn direct_session() -> serde_json::Value {
    json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "direct", "prompt": "look"}})
}

#[tokio::test]
async fn a_session_started_after_its_server_was_ready_gets_the_grant() {
    let f = fx().await;
    let web = person_starts(&f, "web").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    assert!(grants(&f).is_empty(), "no session yet");
    ok(&f.core, direct_session()).await;
    // Sent again once its netproxy runs; the fake ignores earlier grants
    // as the real one does.
    assert_eq!(
        grants(&f),
        BTreeMap::from([("s1".to_owned(), vec![(5173, 1)])])
    );
    insta::assert_json_snapshot!(server_audit(&f));
}

/// Stops `session` and waits until it shows as stopped.
async fn stop_session(f: &Fx, session: &str) {
    let mut rx = f.core.subscribe();
    let stop = json!({"method": "agents.stop", "params": {"session": session}});
    ok(&f.core, stop).await;
    loop {
        match Core::next_event(&mut rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session: s,
                status: AgentStatus::Stopped,
                ..
            })) if s.0 == session => return,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

#[tokio::test]
async fn a_late_end_of_an_earlier_run_leaves_the_resumed_runs_grants() {
    let f = fx().await;
    let web = person_starts(&f, "web").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    ok(&f.core, direct_session()).await;
    let s1 = SessionId("s1".into());
    let first = f.core.sessions().find(&s1, |s| s.run());
    stop_session(&f, "s1").await;
    let resume = json!({"method": "agents.resume", "params": {"session": "s1", "prompt": "again"}});
    ok(&f.core, resume).await;
    assert_eq!(grants(&f)["s1"], [(5173, 1)]);
    // The first run's end arrives only now.
    crate::servers::session_ended(&f.core, &s1, first.unwrap_or_else(|e| panic!("{e}")));
    assert!(f.core.dev_servers().lock().granted.contains_key(&s1));
    let stop = json!({"method": "servers.stop", "params": {"id": web.id.0}});
    ok(&f.core, stop).await;
    assert!(grants(&f).is_empty(), "the resumed run's netproxy lost it");
}

#[tokio::test]
async fn a_workers_servers_end_before_its_changes_land_and_with_it() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let main = person_starts(&f, "web").await;
    let own = starts_in(&f, "web", Some("s2")).await;
    let own = own.unwrap_or_else(|e| panic!("{e}"));
    f.dev.report(&own.id, ServerStatus::Ready);
    let worker = SessionId("s2".into());
    assert!(crate::servers::worktree_stopped(&f.core, &worker).await);
    let stopped = || {
        let stopped = f.dev.stopped.lock().map(|s| s.clone()).unwrap_or_default();
        stopped.into_iter().map(|(id, _)| id.0).collect::<Vec<_>>()
    };
    assert_eq!(stopped(), std::slice::from_ref(&own.id.0));
    starts_in(&f, "web", Some("s2"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    stop_session(&f, "s2").await;
    assert_eq!(stopped(), [own.id.0.clone(), own.id.0.clone()]);
    // An ended worker's worktree gets no new servers, a restart neither.
    let again = starts_in(&f, "web", Some("s2")).await;
    assert_eq!(again.err().map(|e| e.code), Some(ErrorCode::Conflict));
    // The main checkout's server belongs to nobody: it runs on.
    let running = f.dev.running.lock().map(|r| r.contains_key(&main.id));
    assert_eq!(running.ok(), Some(true));
}

#[tokio::test]
async fn the_overview_lists_every_work_dirs_servers() {
    let f = fx().await;
    coordinator_and_worker(&f).await;
    let main = person_starts(&f, "web").await;
    let own = starts_in(&f, "web", Some("s2")).await;
    let own = own.unwrap_or_else(|e| panic!("{e}"));
    let overview = json!({"method": "servers.overview", "params": {"project": "p1"}});
    let Response::Servers(ServersResponse::Overview { servers }) = ok(&f.core, overview).await
    else {
        panic!("not an overview");
    };
    let seen: Vec<_> = servers
        .iter()
        .map(|s| (&s.id, s.work_dir.as_ref()))
        .collect();
    let worker = SessionId("s2".into());
    assert_eq!(seen, [(&main.id, None), (&own.id, Some(&worker))]);
    let other = json!({"method": "servers.overview", "params": {"project": "nope"}});
    let refused = call(&f.core, other).await.err().map(|e| e.code);
    assert_eq!(refused, Some(ErrorCode::NotFound));
}

#[tokio::test(start_paused = true)]
async fn a_worktree_counts_as_stopped_only_once_its_pod_ended() {
    let f = fx().await;
    let dir = coordinator_and_worker(&f).await;
    starts_in(&f, "web", Some("s2"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let worker = SessionId("s2".into());
    // Its server ends, but its pod's process is slow to exit.
    f.dev.hold_pod(dir.clone());
    assert!(!crate::servers::worktree_stopped(&f.core, &worker).await);
    let waiting = crate::servers::worktree_stopped(&f.core, &worker);
    let release = async {
        tokio::task::yield_now().await;
        f.dev.release_pod(&dir);
    };
    let (stopped, ()) = tokio::join!(waiting, release);
    assert!(stopped);
}

#[tokio::test]
async fn an_ended_tasks_worktree_goes_only_after_its_pod_ended_without_making_anyone_wait() {
    let f = fx().await;
    let dir = coordinator_and_worker(&f).await;
    let web = starts_in(&f, "web", Some("s2")).await;
    f.dev.report(
        &web.unwrap_or_else(|e| panic!("{e}")).id,
        ServerStatus::Ready,
    );
    f.dev.hold_pod(dir.clone());
    let stop = json!({"method": "orchestration.stop", "params": {"task": "t1"}});
    ok(&f.core, stop).await;
    stop_session(&f, "s2").await;
    let list = json!({"method": "orchestration.list", "params": {"project": "p1"}});
    // Answered at once, with the worktree kept while its pod runs.
    tokio::time::timeout(std::time::Duration::from_secs(1), ok(&f.core, list.clone()))
        .await
        .unwrap_or_else(|_| panic!("the list waited for the pod"));
    assert!(f.wt.has(&dir));
    f.dev.release_pod(&dir);
    // Its clean-up runs again by itself once the pod ended.
    while f.wt.has(&dir) {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_dial_reaches_only_the_generation_it_was_granted_for() {
    let f = fx().await;
    let web = person_starts(&f, "web").await;
    f.dev.report(&web.id, ServerStatus::Ready);
    let dial = super::grants::dial(&f.core, &web.id, 1, Path::new(ROOT), 5173);
    f.dev.serve(ROOT, 5173, Box::new(tokio::io::duplex(64).0));
    assert!(dial().await.is_ok());
    let restart = json!({"method": "servers.restart", "params": {"id": web.id.0}});
    ok(&f.core, restart).await;
    f.dev.report(&web.id, ServerStatus::Ready);
    f.dev.serve(ROOT, 5173, Box::new(tokio::io::duplex(64).0));
    assert_eq!(
        dial().await.err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
    let current = super::grants::dial(&f.core, &web.id, 2, Path::new(ROOT), 5173);
    assert!(current().await.is_ok());
}

#[tokio::test]
async fn a_bad_port_in_servers_json_forwards_nothing_and_the_session_still_runs() {
    let f = fx().await;
    let bridge = r#"{"servers": [{"name": "web", "command": ["x"], "port": 3128}]}"#;
    f.fs.put(Path::new(ROOT), ".ovra/servers.json", bridge);
    let started = ok(&f.core, direct_session()).await;
    assert!(
        matches!(&started, Response::Agents(AgentsResponse::Session(s)) if !s.status.is_ended()),
        "{started:?}"
    );
    let forward = f.agent.started.lock().map(|s| s[0].sandbox.forward.clone());
    assert_eq!(forward.ok(), Some(Vec::new()));
}

#[tokio::test]
async fn without_a_servers_service_starts_are_unavailable() {
    let fs = Arc::new(MemoryFs::default());
    fs.put(Path::new(ROOT), ".ovra/servers.json", SERVERS);
    let core = Core::builder("t")
        .projects(Arc::new(MemoryProjects::default()))
        .fs(fs)
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let start = json!({"method": "servers.start", "params": {"project": "p1", "name": "web"}});
    assert_eq!(code(call(&core, start).await), ErrorCode::Unavailable);
    // The definitions still show.
    let list = ok(
        &core,
        json!({"method": "servers.list", "params": {"project": "p1"}}),
    )
    .await;
    assert!(
        matches!(list, Response::Servers(ServersResponse::List(l)) if l.definitions.len() == 2)
    );
}

mod tools;
