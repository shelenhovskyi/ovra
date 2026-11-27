//! Tasks that Ovra's quit cut off, continued after the restart
//! (`docs/specs/stuck-and-resume.md`).

use super::*;
use crate::protocol::agents::NotResumed;

/// A core over the same kept projects, sessions and worktrees.
struct Kept {
    projects: Arc<MemoryProjects>,
    sessions: Arc<crate::fakes::MemorySessions>,
    wt: Arc<MemoryWorktrees>,
}

impl Kept {
    fn build(&self) -> Core {
        Core::builder("test")
            .projects(self.projects.clone())
            .sessions(self.sessions.clone())
            .agents(Arc::new(ScriptedAgent::new(say())))
            .worktrees(self.wt.clone())
            .worktrees_dir(PathBuf::from(WORKTREES))
            .build()
    }
}

fn working(core: &Core, session: &str) {
    AgentHost::new(
        sid(session),
        ProjectId("p1".into()),
        core.inner_sessions(),
        core.sink(),
    )
    .status(AgentStatus::Working);
}

async fn listed(core: &Core) -> Vec<crate::protocol::agents::SessionInfo> {
    match ok(
        core,
        json!({"method": "agents.list", "params": {"project": "p1"}}),
    )
    .await
    {
        Response::Agents(AgentsResponse::List(list)) => list,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn coordinators_go_first_and_what_finds_no_place_stays_cut_off() {
    let kept = Kept {
        projects: Arc::new(MemoryProjects::default()),
        sessions: Arc::new(crate::fakes::MemorySessions::default()),
        wt: Arc::new(MemoryWorktrees::default()),
    };
    kept.wt.put(ROOT, "a.rs", "one\n");
    let before = kept.build();
    ok(
        &before,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    let mut rx = before.subscribe();
    // Two tasks: s1 with worker s2, s3 with worker s4.
    for (boss, worker) in [("s1", "s2"), ("s3", "s4")] {
        start_task(&before, 3).await;
        until(&mut rx, boss, AgentStatus::Idle).await;
        let spawned = before
            .spawn_agent(&caller(boss, ToolRole::Coordinator), worker, "x", false)
            .await;
        spawned.unwrap_or_else(|e| panic!("{e}"));
        until(&mut rx, worker, AgentStatus::Idle).await;
    }
    for session in ["s1", "s2", "s3", "s4"] {
        working(&before, session);
    }
    drop(before);

    let after = kept.build();
    ok(
        &after,
        json!({"method": "agents.set_limit", "params": {"limit": 1}}),
    )
    .await;
    assert!(listed(&after).await.iter().all(|s| s.interrupted));
    let mut rx = after.subscribe();
    let answer = ok(
        &after,
        json!({"method": "agents.resume_interrupted", "params": {"project": "p1"}}),
    )
    .await;
    let Response::Agents(AgentsResponse::Resumed { resumed, left }) = answer else {
        panic!("unexpected {answer:?}");
    };
    assert_eq!(resumed, [sid("s1"), sid("s3"), sid("s2")]);
    let [NotResumed { session, reason }] = left.as_slice() else {
        panic!("unexpected {left:?}");
    };
    assert_eq!(session, &sid("s4"));
    assert!(reason.starts_with("every agent place"), "{reason}");
    // They start in that order too, and finish their turns.
    let mut started = Vec::new();
    let mut idle = 0;
    while idle < 3 {
        match Core::next_event(&mut rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session, status, ..
            })) => match status {
                AgentStatus::Starting => started.push(session),
                AgentStatus::Idle => idle += 1,
                _ => {}
            },
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
    assert_eq!(started, resumed);
    drop(after);

    // What could not go on can still, after another restart.
    let again = kept.build();
    let cut_off: Vec<_> = listed(&again)
        .await
        .into_iter()
        .filter(|s| s.interrupted)
        .map(|s| s.id)
        .collect();
    assert_eq!(cut_off, [sid("s4")]);
}
