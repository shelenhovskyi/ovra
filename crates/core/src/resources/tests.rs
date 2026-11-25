use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use super::{CPU_WINDOW, GIB, judge};
use crate::Core;
use crate::fakes::{FakeResources, MemoryProjects, MemoryWorktrees, ScriptedAgent, Step, fake_pid};
use crate::protocol::agents::{AgentStatus, AgentsEvent, SessionId};
use crate::protocol::projects::ProjectId;
use crate::protocol::resources::{Pressure, ResourcesEvent, SessionResources, SystemLoad, Usage};
use crate::protocol::{Event, Request, Response};
use crate::services::{ToolCaller, ToolRole};

const ROOT: &str = crate::test_path!("/work/a");

fn system(total_gib: u64, available_mib: u64) -> SystemLoad {
    SystemLoad {
        memory_total_bytes: total_gib * GIB,
        memory_available_bytes: available_mib << 20,
        cpu_percent: 0,
    }
}

fn cpu(samples: &[u32]) -> VecDeque<u32> {
    samples.iter().copied().collect()
}

#[test]
fn memory_pressure_starts_low_and_ends_higher() {
    let calm = cpu(&[]);
    // 16 GiB: starts below 1.6 GiB (10 %), ends above 2.4 GiB (15 %).
    assert_eq!(judge(None, &system(16, 1700), &calm), None);
    let low = judge(None, &system(16, 1500), &calm);
    assert_eq!(low, Some(Pressure::LowMemory));
    assert_eq!(judge(low, &system(16, 2000), &calm), low);
    assert_eq!(judge(low, &system(16, 2500), &calm), None);
    // 4 GiB: the 1 GiB floor and the 1.5 GiB end matter more.
    assert_eq!(judge(None, &system(4, 900), &calm), low);
    assert_eq!(judge(low, &system(4, 1400), &calm), low);
    assert_eq!(judge(low, &system(4, 1600), &calm), None);
    // A machine that could not tell its memory is never short of it.
    assert_eq!(judge(None, &SystemLoad::default(), &calm), None);
}

#[test]
fn cpu_pressure_needs_thirty_busy_seconds() {
    let memory = system(16, 8000);
    let busy = |n: usize, at: u32| cpu(&vec![at; n]);
    assert_eq!(judge(None, &memory, &busy(CPU_WINDOW - 1, 100)), None);
    let held = judge(None, &memory, &busy(CPU_WINDOW, 90));
    assert_eq!(held, Some(Pressure::BusyCpu));
    assert_eq!(judge(held, &memory, &busy(CPU_WINDOW, 76)), held);
    assert_eq!(judge(held, &memory, &busy(CPU_WINDOW, 74)), None);
    assert_eq!(judge(None, &memory, &busy(CPU_WINDOW, 89)), None);
    // Low memory says more than a busy CPU.
    let both = judge(None, &system(16, 500), &busy(CPU_WINDOW, 100));
    assert_eq!(both, Some(Pressure::LowMemory));
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request)
        .await
        .unwrap_or_else(|e| panic!("request failed: {e}"))
}

/// The next sample of `p1`.
async fn sample(rx: &mut broadcast::Receiver<Event>) -> (Option<Pressure>, Vec<SessionResources>) {
    loop {
        match Core::next_event(rx).await {
            Some(Event::Resources(ResourcesEvent::Sample {
                project,
                pressure,
                sessions,
                ..
            })) if project == ProjectId("p1".into()) => return (pressure, sessions),
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
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

fn status(core: &Core, session: &str) -> AgentStatus {
    core.sessions()
        .find(&SessionId(session.into()), |s| s.status().clone())
        .unwrap_or_else(|e| panic!("{e}"))
}

async fn core_with(resources: Arc<FakeResources>) -> Core {
    let worktrees = Arc::new(MemoryWorktrees::default());
    worktrees.put(ROOT, "a.rs", "one\n");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::new(vec![vec![Step::Say("on it")]])))
        .worktrees(worktrees)
        .worktrees_dir(PathBuf::from(crate::test_path!("/data/worktrees")))
        .resources(resources)
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    core
}

#[tokio::test(start_paused = true)]
async fn running_sessions_are_sampled_and_sampling_stops_with_them() {
    let resources = Arc::new(FakeResources::default());
    resources.set_system(16 * GIB, 8 * GIB, 20);
    let agent = Usage {
        memory_bytes: 400 << 20,
        cpu_percent: 35,
        processes: 4,
    };
    resources.set_tree(fake_pid(&SessionId("s1".into())), agent);
    let core = core_with(resources.clone()).await;
    assert!(
        resources.asked().is_empty(),
        "nothing runs, nothing sampled"
    );
    let mut rx = core.subscribe();
    ok(
        &core,
        json!({"method": "agents.start", "params": {
            "project": "p1", "agent": "claude", "mode": "direct", "prompt": "fix it"}}),
    )
    .await;
    // The first sample may come before the agent reported its process.
    let sessions = loop {
        let (pressure, sessions) = sample(&mut rx).await;
        assert_eq!(pressure, None);
        if sessions.iter().any(|s| s.agent.is_some()) {
            break sessions;
        }
    };
    assert_eq!(
        sessions,
        [SessionResources {
            session: SessionId("s1".into()),
            agent: Some(agent),
            servers: None,
            browser: None,
        }]
    );

    ok(
        &core,
        json!({"method": "agents.stop", "params": {"session": "s1"}}),
    )
    .await;
    // A last, empty sample clears the project's lines.
    loop {
        let (pressure, sessions) = sample(&mut rx).await;
        if sessions.is_empty() {
            assert_eq!(pressure, None);
            break;
        }
    }
    while super::sampling(&core) {
        tokio::time::advance(super::INTERVAL).await;
        tokio::task::yield_now().await;
    }
    let asked = resources.asked().len();
    for _ in 0..5 {
        tokio::time::advance(super::INTERVAL).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(resources.asked().len(), asked, "no session, no samples");
}

#[tokio::test(start_paused = true)]
async fn workers_wait_while_memory_is_low_and_then_start_one_at_a_time() {
    let resources = Arc::new(FakeResources::default());
    resources.set_system(16 * GIB, GIB / 2, 10);
    let core = core_with(resources.clone()).await;
    let mut rx = core.subscribe();
    let task = ok(
        &core,
        json!({"method": "orchestration.start", "params": {
            "project": "p1", "prompt": "split the work",
            "limits": {"max_workers": 3, "budget_micro_usd": 1_000_000}}}),
    )
    .await;
    assert!(matches!(task, Response::Orchestration(_)));
    assert_eq!(sample(&mut rx).await.0, Some(Pressure::LowMemory));

    // A free place, but no memory: both wait.
    let boss = ToolCaller {
        session: SessionId("s1".into()),
        project: ProjectId("p1".into()),
        role: ToolRole::Coordinator,
    };
    for name in ["one", "two"] {
        let worker = core.spawn_agent(&boss, name, "x", false).await;
        assert!(worker.is_ok(), "{worker:?}");
    }
    assert_eq!(status(&core, "s2"), AgentStatus::Queued);
    assert_eq!(status(&core, "s3"), AgentStatus::Queued);
    assert_eq!(sample(&mut rx).await.0, Some(Pressure::LowMemory));
    assert_eq!(status(&core, "s2"), AgentStatus::Queued);

    // Memory is back: the oldest starts with this sample, the next with
    // the one after.
    resources.set_system(16 * GIB, 8 * GIB, 10);
    assert_eq!(sample(&mut rx).await.0, None);
    until(&mut rx, "s2", AgentStatus::Idle).await;
    assert_eq!(status(&core, "s3"), AgentStatus::Queued);
    assert_eq!(sample(&mut rx).await.0, None);
    until(&mut rx, "s3", AgentStatus::Idle).await;
}

#[tokio::test(start_paused = true)]
async fn after_a_shortage_a_worker_waiting_only_for_a_place_does_not_slow_the_rest() {
    let resources = Arc::new(FakeResources::default());
    resources.set_system(16 * GIB, GIB / 2, 10);
    let core = core_with(resources.clone()).await;
    let mut rx = core.subscribe();
    ok(
        &core,
        json!({"method": "orchestration.start", "params": {
            "project": "p1", "prompt": "split the work",
            "limits": {"max_workers": 4, "budget_micro_usd": 1_000_000}}}),
    )
    .await;
    assert_eq!(sample(&mut rx).await.0, Some(Pressure::LowMemory));
    let boss = ToolCaller {
        session: SessionId("s1".into()),
        project: ProjectId("p1".into()),
        role: ToolRole::Coordinator,
    };
    for name in ["one", "two", "three", "four"] {
        assert!(core.spawn_agent(&boss, name, "x", false).await.is_ok());
    }
    resources.set_system(16 * GIB, 8 * GIB, 10);
    for worker in ["s2", "s3", "s4"] {
        until(&mut rx, worker, AgentStatus::Idle).await;
    }
    // The fourth waits for a place now, not for memory: the ramp ends.
    for _ in 0..3 {
        sample(&mut rx).await;
    }
    assert!(!super::holds_back(&core));
    assert_eq!(status(&core, "s5"), AgentStatus::Queued);
}
