use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;

use super::*;
use crate::fakes::{MemoryProjects, MemoryWorktrees, ScriptedAgent, Step};
use crate::protocol::agents::{AgentKind, AgentsEvent, AgentsResponse, SessionInfo};
use crate::protocol::{Event, Request};

const ROOT: &str = crate::test_path!("/work/a");
const WORKTREES: &str = crate::test_path!("/data/worktrees");
/// 2026-11-27T10:00:00Z, a Friday.
const NOW: u64 = 1_795_773_600_000;
const MINUTE: u64 = 60_000;

struct Fx {
    core: Core,
    wt: Arc<MemoryWorktrees>,
}

async fn fx(turns: Vec<Vec<Step>>) -> Fx {
    let wt = Arc::new(MemoryWorktrees::default());
    wt.put(ROOT, "a.rs", "one\n");
    let core = Core::builder("test")
        .projects(Arc::new(MemoryProjects::default()))
        .agents(Arc::new(ScriptedAgent::new(turns)))
        .worktrees(wt.clone())
        .worktrees_dir(PathBuf::from(WORKTREES))
        .build();
    ok(
        &core,
        json!({"method": "projects.add", "params": {"root": ROOT}}),
    )
    .await;
    Fx { core, wt }
}

async fn call(core: &Core, json: serde_json::Value) -> Result<Response> {
    let request: Request = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    core.handle(request).await
}

async fn ok(core: &Core, json: serde_json::Value) -> Response {
    call(core, json).await.unwrap_or_else(|e| panic!("{e}"))
}

fn draft(kind: &str) -> serde_json::Value {
    json!({"name": " Bugs ", "prompt": "Look for bugs", "kind": kind,
        "when": {"days": [5, 1, 5], "time": "03:00", "time_zone": "UTC"}})
}

async fn save(core: &Core, kind: &str) -> Schedule {
    match ok(
        core,
        json!({"method": "schedules.save", "params": {"project": "p1", "draft": draft(kind)}}),
    )
    .await
    {
        Response::Schedules(SchedulesResponse::Schedule(s)) => *s,
        other => panic!("unexpected {other:?}"),
    }
}

async fn enable(core: &Core) {
    ok(
        core,
        json!({"method": "schedules.set_enabled", "params": {"enabled": true}}),
    )
    .await;
}

/// Makes `id` due at `at`, as if its time had come.
fn due_at(core: &Core, id: &ScheduleId, at: u64) {
    let _ = change(core, |s| {
        if let Some(x) = s.schedules.iter_mut().find(|x| &x.id == id) {
            x.next_ms = Some(at);
        }
        Ok(())
    });
}

fn stored(core: &Core, id: &ScheduleId) -> Schedule {
    find(core, id).unwrap_or_else(|e| panic!("{e}"))
}

fn sessions(core: &Core) -> Vec<SessionInfo> {
    core.sessions().list(&ProjectId("p1".into()))
}

async fn until(rx: &mut broadcast::Receiver<Event>, session: &SessionId, want: AgentStatus) {
    loop {
        match Core::next_event(rx).await {
            Some(Event::Agents(AgentsEvent::Status {
                session: s, status, ..
            })) if &s == session && status == want => return,
            Some(_) => {}
            None => panic!("core closed"),
        }
    }
}

fn started(schedule: &Schedule) -> SessionId {
    match &schedule.last {
        Some(LastRun {
            outcome: RunOutcome::Started { session },
            ..
        }) => session.clone(),
        other => panic!("not started: {other:?}"),
    }
}

#[tokio::test]
async fn drafts_are_checked_and_kept_trimmed_with_their_days_once_each() {
    let f = fx(vec![]).await;
    let saved = save(&f.core, "research").await;
    assert_eq!(saved.draft.name, "Bugs");
    assert_eq!(saved.draft.when.days, vec![1, 5]);
    assert_eq!(saved.draft.agent, AgentKind::Claude);
    // Off by default: no next run is shown, though one is kept.
    assert_eq!(saved.next_ms, None);
    assert!(stored(&f.core, &saved.id).next_ms.is_some());
    let bad = [
        json!({"name": " ", "prompt": "x", "when": {"days": [1], "time": "03:00", "time_zone": "UTC"}}),
        json!({"name": "a", "prompt": " ", "when": {"days": [1], "time": "03:00", "time_zone": "UTC"}}),
        json!({"name": "a", "prompt": "x", "when": {"days": [], "time": "03:00", "time_zone": "UTC"}}),
        json!({"name": "a", "prompt": "x", "when": {"days": [8], "time": "03:00", "time_zone": "UTC"}}),
        json!({"name": "a", "prompt": "x", "when": {"days": [1], "time": "3:00", "time_zone": "UTC"}}),
        json!({"name": "a", "prompt": "x", "when": {"days": [1], "time": "03:00", "time_zone": "Mars"}}),
    ];
    for draft in bad {
        let save = json!({"method": "schedules.save", "params": {"project": "p1", "draft": draft}});
        let err = call(&f.core, save.clone()).await.err();
        assert_eq!(err.map(|e| e.code), Some(ErrorCode::BadRequest), "{save}");
    }
    let unknown = json!({"method": "schedules.save", "params": {"project": "p9", "draft": draft("research")}});
    assert!(call(&f.core, unknown).await.is_err());
}

#[tokio::test]
async fn a_due_research_schedule_starts_a_session_marked_with_its_name() {
    let f = fx(vec![vec![Step::Say("found two")]]).await;
    let schedule = save(&f.core, "research").await;
    due_at(&f.core, &schedule.id, NOW - MINUTE);
    // Off: nothing runs.
    tick(&f.core, NOW).await;
    assert!(sessions(&f.core).is_empty());
    enable(&f.core).await;
    due_at(&f.core, &schedule.id, NOW - MINUTE);
    // Paused: nothing runs either.
    ok(
        &f.core,
        json!({"method": "schedules.set_paused", "params": {"id": schedule.id.0, "paused": true}}),
    )
    .await;
    tick(&f.core, NOW).await;
    assert!(sessions(&f.core).is_empty());
    ok(
        &f.core,
        json!({"method": "schedules.set_paused", "params": {"id": schedule.id.0, "paused": false}}),
    )
    .await;
    due_at(&f.core, &schedule.id, NOW - MINUTE);

    let mut rx = f.core.subscribe();
    tick(&f.core, NOW).await;
    let after = stored(&f.core, &schedule.id);
    let session = started(&after);
    let [info] = sessions(&f.core)
        .try_into()
        .unwrap_or_else(|s| panic!("{s:?}"));
    assert_eq!(info.id, session);
    assert_eq!(info.mode, SessionMode::Research);
    assert_eq!(info.schedule.as_deref(), Some("Bugs"));
    // The next run: Monday 03:00 UTC after Friday 10:00.
    assert_eq!(after.next_ms, Some(NOW + (2 * 24 + 17) * 60 * MINUTE));
    until(&mut rx, &session, AgentStatus::Idle).await;
    // Not due again until then.
    tick(&f.core, NOW + MINUTE).await;
    assert_eq!(sessions(&f.core).len(), 1);
}

#[tokio::test]
async fn a_run_more_than_an_hour_late_is_missed_and_not_started() {
    let f = fx(vec![]).await;
    let schedule = save(&f.core, "research").await;
    enable(&f.core).await;
    due_at(&f.core, &schedule.id, NOW - 61 * MINUTE);
    tick(&f.core, NOW).await;
    assert!(sessions(&f.core).is_empty());
    let after = stored(&f.core, &schedule.id);
    assert_eq!(after.last.map(|l| l.outcome), Some(RunOutcome::Missed));
    assert!(after.next_ms.is_some_and(|n| n > NOW));
}

#[tokio::test]
async fn full_research_places_keep_it_due_and_skip_it_after_the_hour() {
    let f = fx(vec![vec![Step::Say("reading")]]).await;
    ok(
        &f.core,
        json!({"method": "agents.set_research_limit", "params": {"limit": 1}}),
    )
    .await;
    let schedule = save(&f.core, "research").await;
    enable(&f.core).await;
    // Another research session takes the one place.
    let other = json!({"method": "agents.start", "params": {
        "project": "p1", "agent": "claude", "mode": "research", "prompt": "Read"}});
    let Response::Agents(AgentsResponse::Session(other)) = ok(&f.core, other).await else {
        panic!("no session");
    };
    assert!(still_works(&f.core, &other.id));
    due_at(&f.core, &schedule.id, NOW);
    tick(&f.core, NOW).await;
    let waiting = stored(&f.core, &schedule.id);
    assert_eq!((waiting.next_ms, waiting.last), (Some(NOW), None));
    tick(&f.core, NOW + 61 * MINUTE).await;
    let after = stored(&f.core, &schedule.id);
    assert_eq!(
        after.last.map(|l| l.outcome),
        Some(RunOutcome::Skipped {
            reason: "1 of 1 research agents were working.".into()
        })
    );
    assert_eq!(sessions(&f.core).len(), 1);
}

#[tokio::test]
async fn research_turned_off_fails_the_run_which_is_not_started_again() {
    let f = fx(vec![]).await;
    ok(
        &f.core,
        json!({"method": "agents.set_research_limit", "params": {"limit": 0}}),
    )
    .await;
    let schedule = save(&f.core, "research").await;
    enable(&f.core).await;
    due_at(&f.core, &schedule.id, NOW);
    tick(&f.core, NOW).await;
    let after = stored(&f.core, &schedule.id);
    assert_eq!(
        after.last.map(|l| l.outcome),
        Some(RunOutcome::Failed {
            message: "Research agents are turned off in Settings.".into()
        })
    );
    // As if that record had not been saved: the due time was handled, so
    // it is not tried again.
    due_at(&f.core, &schedule.id, NOW);
    let _ = change(&f.core, |s| {
        s.schedules[0].last = None;
        Ok(())
    });
    tick(&f.core, NOW + MINUTE).await;
    let after = stored(&f.core, &schedule.id);
    assert_eq!(after.last, None);
    // Only its next run moved on.
    assert!(after.next_ms.is_some_and(|n| n > NOW + MINUTE));
}

#[tokio::test]
async fn run_now_takes_a_run_due_by_the_next_tick_and_the_clock_skips_while_it_works() {
    let f = fx(vec![vec![Step::Say("one")]]).await;
    ok(
        &f.core,
        json!({"method": "agents.set_research_limit", "params": {"limit": 2}}),
    )
    .await;
    let schedule = save(&f.core, "research").await;
    enable(&f.core).await;
    let soon = agents::now_ms() + MINUTE / 6;
    due_at(&f.core, &schedule.id, soon);
    let Response::Schedules(SchedulesResponse::Schedule(ran)) = ok(
        &f.core,
        json!({"method": "schedules.run_now", "params": {"id": schedule.id.0}}),
    )
    .await
    else {
        panic!("no schedule");
    };
    let first = started(&ran);
    assert!(stored(&f.core, &schedule.id).next_ms > Some(soon));
    // Due again while that run still starts: skipped, not a second session.
    assert!(still_works(&f.core, &first));
    due_at(&f.core, &schedule.id, NOW);
    tick(&f.core, NOW).await;
    match stored(&f.core, &schedule.id).last.map(|l| l.outcome) {
        Some(RunOutcome::Skipped { reason }) => {
            assert_eq!(reason, "The last run is still working.")
        }
        other => panic!("not skipped: {other:?}"),
    }
    assert_eq!(stored(&f.core, &schedule.id).last_session, Some(first));
    assert_eq!(sessions(&f.core).len(), 1);
}

#[test]
fn a_schedule_that_cannot_be_read_does_not_hide_the_other_settings() {
    let json = json!({"claude_path": "/opt/claude", "schedules": [
        {"id": "sc1", "project": "p1", "name": "Old", "prompt": "x",
            "when": {"days": [1], "time": "03:00", "time_zone": "UTC"}},
        {"id": "sc2", "broken": true},
    ]});
    let settings: AgentSettings = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(settings.claude_path.as_deref(), Some("/opt/claude"));
    assert_eq!(settings.schedules.len(), 1);
}

#[tokio::test]
async fn an_own_copy_run_is_skipped_while_the_last_works_or_waits_in_reviews() {
    // The first run asks to use Bash and waits for the person.
    let f = fx(vec![vec![Step::Use {
        tool: "Bash",
        input: json!({"command": "ls"}),
    }]])
    .await;
    let schedule = save(&f.core, "own_copy").await;
    enable(&f.core).await;
    let mut rx = f.core.subscribe();
    due_at(&f.core, &schedule.id, NOW);
    tick(&f.core, NOW).await;
    let first = started(&stored(&f.core, &schedule.id));
    let task = sessions(&f.core);
    assert_eq!(task.len(), 1);
    assert_eq!(task[0].mode, SessionMode::Isolated);
    assert_eq!(task[0].schedule.as_deref(), Some("Bugs"));
    until(&mut rx, &first, AgentStatus::WaitingForApproval).await;

    due_at(&f.core, &schedule.id, NOW + MINUTE);
    tick(&f.core, NOW + MINUTE).await;
    let skipped = |s: Schedule| match s.last.map(|l| l.outcome) {
        Some(RunOutcome::Skipped { reason }) => reason,
        other => panic!("not skipped: {other:?}"),
    };
    assert_eq!(
        skipped(stored(&f.core, &schedule.id)),
        "The last run is still working."
    );

    // Once it is idle with changes, they wait in Reviews.
    let worktree = core_worktree(&f.core, &first);
    f.wt.put(&worktree, "a.rs", "two\n");
    let mut rx = f.core.subscribe();
    ok(
        &f.core,
        json!({"method": "agents.interrupt", "params": {"session": first.0}}),
    )
    .await;
    until(&mut rx, &first, AgentStatus::Idle).await;
    let Response::Schedules(SchedulesResponse::Schedule(now)) = ok(
        &f.core,
        json!({"method": "schedules.run_now", "params": {"id": schedule.id.0}}),
    )
    .await
    else {
        panic!("no schedule");
    };
    assert_eq!(
        skipped(*now),
        "The last run's changes still wait in Reviews."
    );
    assert_eq!(sessions(&f.core).len(), 1);
}

fn core_worktree(core: &Core, session: &SessionId) -> String {
    core.sessions()
        .find(session, |s| s.launch())
        .ok()
        .flatten()
        .map(|l| l.cwd.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tokio::test]
async fn run_now_works_while_off_and_keeps_the_next_run() {
    let f = fx(vec![vec![Step::Say("done")]]).await;
    let schedule = save(&f.core, "research").await;
    let next = stored(&f.core, &schedule.id).next_ms;
    let Response::Schedules(SchedulesResponse::Schedule(ran)) = ok(
        &f.core,
        json!({"method": "schedules.run_now", "params": {"id": schedule.id.0}}),
    )
    .await
    else {
        panic!("no schedule");
    };
    started(&ran);
    assert_eq!(stored(&f.core, &schedule.id).next_ms, next);
    let Response::Schedules(SchedulesResponse::List(list)) = ok(
        &f.core,
        json!({"method": "schedules.list", "params": {"project": "p1"}}),
    )
    .await
    else {
        panic!("no list");
    };
    assert_eq!(list.len(), 1);
    ok(
        &f.core,
        json!({"method": "schedules.remove", "params": {"id": schedule.id.0}}),
    )
    .await;
    assert!(find(&f.core, &schedule.id).is_err());
    assert!(matches!(
        ok(&f.core, json!({"method": "agents.list", "params": {"project": "p1"}})).await,
        Response::Agents(AgentsResponse::List(l)) if l.len() == 1
    ));
}

#[tokio::test]
async fn turning_them_on_skips_times_that_passed_while_off() {
    let f = fx(vec![]).await;
    let schedule = save(&f.core, "research").await;
    due_at(&f.core, &schedule.id, 1_000);
    enable(&f.core).await;
    assert!(
        stored(&f.core, &schedule.id)
            .next_ms
            .is_some_and(|n| n > agents::now_ms())
    );
    // A removed project takes its schedules along.
    ok(
        &f.core,
        json!({"method": "projects.remove", "params": {"id": "p1"}}),
    )
    .await;
    assert!(find(&f.core, &schedule.id).is_err());
}

#[tokio::test]
async fn a_remote_core_without_a_client_starts_nothing_scheduled_until_one_attaches() {
    let f = fx(vec![vec![Step::Say("found two")]]).await;
    let schedule = save(&f.core, "research").await;
    enable(&f.core).await;
    due_at(&f.core, &schedule.id, NOW - MINUTE);
    f.core.set_attended(false);
    tick(&f.core, NOW).await;
    assert!(sessions(&f.core).is_empty());
    assert!(!f.core.runs_anything());
    f.core.set_attended(true);
    tick(&f.core, NOW).await;
    assert_eq!(sessions(&f.core).len(), 1);
}
