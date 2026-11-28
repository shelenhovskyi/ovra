//! Schedules (ADR 0017, `docs/specs/schedules.md`): prompts the person
//! saved that start a research session or one agent in its own copy at set
//! days and times, while Ovra runs and only once the person turned them on.
//! They are kept in [`AgentSettings`]; a clock task fires the due ones.

mod time;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::Core;
use crate::agents::{self, NewSession};
use crate::core::WeakCore;
use crate::orchestration::{self, Solo};
use crate::protocol::agents::{AgentSettings, AgentStatus, MessageFrom, SessionId, SessionMode};
use crate::protocol::projects::ProjectId;
use crate::protocol::schedules::{
    LastRun, RunOutcome, Schedule, ScheduleDraft, ScheduleId, ScheduleKind, SchedulesEvent,
    SchedulesRequest, SchedulesResponse,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};

type Result<T> = std::result::Result<T, ProtocolError>;

/// How often the clock looks for due schedules.
const TICK: Duration = Duration::from_secs(30);
const TICK_MS: u64 = 30_000;
/// How late a run may start; later, it is missed.
const LATE_MS: u64 = 60 * 60 * 1000;
/// Schedules one project may have.
const MAX_PER_PROJECT: usize = 20;
const MAX_NAME_CHARS: usize = 80;

/// The clock's state; schedules themselves are in the settings.
#[derive(Debug, Default)]
pub(crate) struct State {
    started: AtomicBool,
    /// Held by the clock's whole tick and by every change of schedules,
    /// so that a schedule paused, changed, removed or switched off is never
    /// started from what the tick read before.
    firing: tokio::sync::Mutex<()>,
    /// The due time each schedule last started (or gave up) a run for, so
    /// that a run whose record failed to save is not started again.
    fired: Mutex<std::collections::BTreeMap<ScheduleId, u64>>,
    /// Why a due run could not start yet (the research places are full),
    /// by schedule; its outcome if the hour passes. Not saved.
    busy: Mutex<std::collections::BTreeMap<ScheduleId, String>>,
    /// Set while a remote core has no client: nothing scheduled starts
    /// then (`docs/specs/ssh-remotes.md`, section 6).
    pub(crate) unattended: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the clock once, when a runtime is there to run it.
pub(crate) fn start_clock(core: &Core) {
    let state = core.schedules();
    if state.started.load(Ordering::Relaxed) {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if state.started.swap(true, Ordering::Relaxed) {
        return;
    }
    runtime.spawn(clock(core.downgrade()));
}

async fn clock(weak: WeakCore) {
    loop {
        tokio::time::sleep(TICK).await;
        let Some(core) = weak.upgrade() else {
            return;
        };
        tick(&core, agents::now_ms()).await;
    }
}

/// Changes the settings under the lock every writer of them takes.
fn change<T>(core: &Core, f: impl FnOnce(&mut AgentSettings) -> Result<T>) -> Result<T> {
    agents::change_settings(core.project_store(), f)
}

/// As clients see it: no next run while paused or switched off.
fn view(schedule: &Schedule, enabled: bool) -> Schedule {
    Schedule {
        next_ms: schedule.next_ms.filter(|_| enabled && !schedule.paused),
        ..schedule.clone()
    }
}

fn not_found(id: &ScheduleId) -> ProtocolError {
    ProtocolError::new(ErrorCode::NotFound, format!("schedule {}", id.0))
}

fn bad(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::BadRequest, message)
}

/// The draft as it is kept: trimmed, days sorted once each.
fn checked(draft: ScheduleDraft) -> Result<ScheduleDraft> {
    let name = draft.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("the schedule needs a name"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(bad("the name is longer than 80 characters"));
    }
    agents::check_text(&draft.prompt)?;
    let mut when = draft.when;
    when.days.sort_unstable();
    when.days.dedup();
    if when.days.is_empty() || when.days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(bad("choose at least one day, Monday (1) to Sunday (7)"));
    }
    if time::parse(&when.time).is_none() {
        return Err(bad("the time must be HH:MM, from 00:00 to 23:59"));
    }
    if !time::known_zone(&when.time_zone) {
        return Err(bad("the time zone is not known"));
    }
    Ok(ScheduleDraft {
        name,
        when,
        ..draft
    })
}

pub(crate) async fn handle(core: &Core, request: SchedulesRequest) -> Result<Response> {
    let reply = |r| Ok(Response::Schedules(r));
    match request {
        SchedulesRequest::Enabled => {
            let enabled = core.project_store().agent_settings()?.schedules_enabled;
            reply(SchedulesResponse::Enabled { enabled })
        }
        SchedulesRequest::SetEnabled { enabled } => {
            let _firing = core.schedules().firing.lock().await;
            let now = agents::now_ms();
            change(core, |s| {
                if enabled && !s.schedules_enabled {
                    // Times that passed while they were off are not runs.
                    for schedule in &mut s.schedules {
                        schedule.next_ms = time::next_after(&schedule.draft.when, now);
                    }
                }
                s.schedules_enabled = enabled;
                Ok(())
            })?;
            lock(&core.schedules().busy).clear();
            core.emit(SchedulesEvent::Changed { project: None });
            reply(SchedulesResponse::Enabled { enabled })
        }
        SchedulesRequest::List { project } => {
            core.project_store().get(&project)?;
            let settings = core.project_store().agent_settings()?;
            let list = settings
                .schedules
                .iter()
                .filter(|s| s.project == project)
                .map(|s| view(s, settings.schedules_enabled))
                .collect();
            reply(SchedulesResponse::List(list))
        }
        SchedulesRequest::Save { project, id, draft } => {
            let _firing = core.schedules().firing.lock().await;
            core.project_store().get(&project)?;
            let draft = checked(draft)?;
            let next_ms = time::next_after(&draft.when, agents::now_ms());
            let (saved, enabled) = change(core, |s| {
                let saved = match &id {
                    Some(id) => {
                        let schedule = s
                            .schedules
                            .iter_mut()
                            .find(|x| &x.id == id && x.project == project)
                            .ok_or_else(|| not_found(id))?;
                        schedule.draft = draft;
                        schedule.next_ms = next_ms;
                        schedule.clone()
                    }
                    None => {
                        let count = s.schedules.iter().filter(|x| x.project == project).count();
                        if count >= MAX_PER_PROJECT {
                            return Err(bad("a project can have at most 20 schedules"));
                        }
                        s.schedules_made += 1;
                        let schedule = Schedule {
                            id: ScheduleId(format!("sc{}", s.schedules_made)),
                            project: project.clone(),
                            draft,
                            paused: false,
                            next_ms,
                            last: None,
                            last_session: None,
                        };
                        s.schedules.push(schedule.clone());
                        schedule
                    }
                };
                Ok((saved, s.schedules_enabled))
            })?;
            core.emit(SchedulesEvent::Changed {
                project: Some(project),
            });
            reply(SchedulesResponse::Schedule(Box::new(view(&saved, enabled))))
        }
        SchedulesRequest::SetPaused { id, paused } => {
            let _firing = core.schedules().firing.lock().await;
            let now = agents::now_ms();
            let (saved, enabled) = change(core, |s| {
                let schedule = s
                    .schedules
                    .iter_mut()
                    .find(|x| x.id == id)
                    .ok_or_else(|| not_found(&id))?;
                if schedule.paused && !paused {
                    schedule.next_ms = time::next_after(&schedule.draft.when, now);
                }
                schedule.paused = paused;
                Ok((schedule.clone(), s.schedules_enabled))
            })?;
            core.emit(SchedulesEvent::Changed {
                project: Some(saved.project.clone()),
            });
            reply(SchedulesResponse::Schedule(Box::new(view(&saved, enabled))))
        }
        SchedulesRequest::Remove { id } => {
            let _firing = core.schedules().firing.lock().await;
            let project = change(core, |s| {
                let at = s
                    .schedules
                    .iter()
                    .position(|x| x.id == id)
                    .ok_or_else(|| not_found(&id))?;
                Ok(s.schedules.remove(at).project)
            })?;
            lock(&core.schedules().busy).remove(&id);
            lock(&core.schedules().fired).remove(&id);
            core.emit(SchedulesEvent::Changed {
                project: Some(project),
            });
            Ok(Response::Ok(OkResponse::Ok))
        }
        SchedulesRequest::RunNow { id } => {
            let _firing = core.schedules().firing.lock().await;
            let schedule = find(core, &id)?;
            let now = agents::now_ms();
            let (outcome, next) = match run(core, &schedule).await {
                // A run due now, or by the next tick, is this one.
                Run::Done(outcome @ RunOutcome::Started { .. })
                    if schedule.next_ms.is_some_and(|at| at <= now + TICK_MS) =>
                {
                    lock(&core.schedules().busy).remove(&id);
                    (outcome, Some(time::next_after(&schedule.draft.when, now)))
                }
                Run::Done(outcome) => (outcome, None),
                Run::Busy(reason) => (RunOutcome::Skipped { reason }, None),
            };
            let saved = record(core, &id, outcome, next)?;
            let enabled = core.project_store().agent_settings()?.schedules_enabled;
            reply(SchedulesResponse::Schedule(Box::new(view(&saved, enabled))))
        }
    }
}

/// Sets when the next run is due, keeping the last run as it was.
fn record_next(core: &Core, id: &ScheduleId, next: Option<u64>) -> Result<()> {
    change(core, |s| {
        if let Some(schedule) = s.schedules.iter_mut().find(|x| &x.id == id) {
            schedule.next_ms = next;
        }
        Ok(())
    })
}

fn find(core: &Core, id: &ScheduleId) -> Result<Schedule> {
    core.project_store()
        .agent_settings()?
        .schedules
        .into_iter()
        .find(|s| &s.id == id)
        .ok_or_else(|| not_found(id))
}

/// Keeps how a run went and, with `next`, when the next is due; tells
/// clients. A schedule removed meanwhile is `not_found`.
fn record(
    core: &Core,
    id: &ScheduleId,
    outcome: RunOutcome,
    next: Option<Option<u64>>,
) -> Result<Schedule> {
    let saved = change(core, |s| {
        let schedule = s
            .schedules
            .iter_mut()
            .find(|x| &x.id == id)
            .ok_or_else(|| not_found(id))?;
        if let RunOutcome::Started { session } = &outcome {
            schedule.last_session = Some(session.clone());
        }
        schedule.last = Some(LastRun {
            at_ms: agents::now_ms(),
            outcome,
        });
        if let Some(next) = next {
            schedule.next_ms = next;
        }
        Ok(schedule.clone())
    })?;
    core.emit(SchedulesEvent::Changed {
        project: Some(saved.project.clone()),
    });
    Ok(saved)
}

/// Fires every schedule due at `now`: started, skipped or, an hour late,
/// missed. A run that waits for a research place stays due.
pub(crate) async fn tick(core: &Core, now: u64) {
    if core.schedules().unattended.load(Ordering::Relaxed) {
        return;
    }
    let _firing = core.schedules().firing.lock().await;
    let Ok(settings) = core.project_store().agent_settings() else {
        return;
    };
    if !settings.schedules_enabled {
        return;
    }
    let due = settings
        .schedules
        .into_iter()
        .filter(|s| !s.paused && s.next_ms.is_some_and(|at| at <= now));
    for schedule in due {
        let at = schedule.next_ms.unwrap_or(now);
        let next = Some(time::next_after(&schedule.draft.when, now));
        let state = core.schedules();
        if lock(&state.fired).get(&schedule.id) == Some(&at) {
            // Handled already, but its record could not be saved: only
            // move on to the next run.
            if let Err(err) = record_next(core, &schedule.id, next.flatten()) {
                tracing::warn!(schedule = %schedule.id.0, "could not save a schedule's next run: {}", err.message);
            }
            continue;
        }
        let outcome = if now.saturating_sub(at) > LATE_MS {
            let busy = lock(&state.busy).remove(&schedule.id);
            busy.map_or(RunOutcome::Missed, |reason| RunOutcome::Skipped { reason })
        } else {
            match run(core, &schedule).await {
                Run::Done(outcome) => outcome,
                Run::Busy(reason) => {
                    lock(&state.busy).insert(schedule.id.clone(), reason);
                    continue;
                }
            }
        };
        lock(&state.busy).remove(&schedule.id);
        lock(&state.fired).insert(schedule.id.clone(), at);
        if let Err(err) = record(core, &schedule.id, outcome, next) {
            tracing::warn!(schedule = %schedule.id.0, "could not save a schedule's run: {}", err.message);
        }
    }
}

enum Run {
    Done(RunOutcome),
    /// Not now, maybe within the hour.
    Busy(String),
}

/// Starts one run of `schedule`, as the person's start would, unless its
/// last run still works or still waits in Reviews.
async fn run(core: &Core, schedule: &Schedule) -> Run {
    let skipped = |reason: &str| {
        Run::Done(RunOutcome::Skipped {
            reason: reason.to_owned(),
        })
    };
    if let Some(session) = &schedule.last_session {
        if still_works(core, session) {
            return skipped("The last run is still working.");
        }
        if schedule.draft.kind == ScheduleKind::OwnCopy
            && orchestration::waits_for_review(core, session).await
        {
            return skipped("The last run's changes still wait in Reviews.");
        }
    }
    let draft = &schedule.draft;
    let project = schedule.project.clone();
    let started = match draft.kind {
        ScheduleKind::Research => {
            let room = orchestration::capacity(core, &project);
            if room.research_limit == 0 {
                return Run::Done(RunOutcome::Failed {
                    message: "Research agents are turned off in Settings.".to_owned(),
                });
            }
            if room.research_working >= room.research_limit {
                return Run::Busy(format!(
                    "{} of {} research agents were working.",
                    room.research_working, room.research_limit
                ));
            }
            let new = NewSession {
                project,
                agent: draft.agent,
                mode: SessionMode::Research,
                prompt: draft.prompt.clone(),
                context: None,
                cwd: None,
                tools: None,
                // The person wrote it when they saved the schedule.
                from: MessageFrom::Person,
                account: None,
                queued: false,
                preparing: false,
                no_web: false,
                model: None,
                plan_first: false,
                schedule: Some(draft.name.clone()),
            };
            agents::start(&core.agents_ctx(), new).await.map(|s| s.id)
        }
        ScheduleKind::OwnCopy => {
            let solo = Solo {
                agent: draft.agent,
                attempt_of: None,
                model: None,
                plan_first: false,
                schedule: Some(draft.name.clone()),
                repos: Vec::new(),
            };
            orchestration::start_solo(core, project, draft.prompt.clone(), None, solo)
                .await
                .and_then(|task| {
                    task.workers
                        .first()
                        .map(|w| w.session.clone())
                        .ok_or_else(|| {
                            ProtocolError::new(ErrorCode::Internal, "the agent did not start")
                        })
                })
        }
    };
    Run::Done(match started {
        Ok(session) => RunOutcome::Started { session },
        Err(err) => RunOutcome::Failed {
            message: err.message,
        },
    })
}

/// Whether `session` has not finished its turn: it waits, starts, works or
/// waits for an approval.
fn still_works(core: &Core, session: &SessionId) -> bool {
    core.sessions()
        .find(session, |s| {
            !matches!(
                s.status(),
                AgentStatus::Idle | AgentStatus::Stopped | AgentStatus::Failed { .. }
            )
        })
        .unwrap_or(false)
}

/// Forgets the schedules of a removed project.
pub(crate) fn forget_project(core: &Core, project: &ProjectId) {
    let _ = change(core, |s| {
        s.schedules.retain(|x| &x.project != project);
        Ok(())
    });
}

#[cfg(test)]
mod tests;
