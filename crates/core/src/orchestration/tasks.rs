//! Tasks: one coordinator and the workers it starts, within limits.

use std::path::PathBuf;

use super::{Result, Workspace, conflict, forbidden, not_found, prepare, review, slots};
use crate::Core;
use crate::agents::{self, NewSession};
use crate::protocol::agents::{
    AgentKind, AgentModel, AgentStatus, MessageFrom, SessionId, SessionMode,
};
use crate::protocol::orchestration::{
    FailedWorker, OrchestrationEvent, OrchestrationRequest, OrchestrationResponse, Task, TaskId,
    TaskLimits, Worker,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{IncludedFile, ToolRole};

/// Most workers a task may run at once, whatever it asks for: each is a
/// `claude` process with its own worktree.
const MAX_WORKERS_CAP: u32 = 16;

/// A task may start at most this many times `max_workers` workers over its
/// life: each one is a branch and a worktree, and spawns need no approval.
const SPAWNS_PER_PLACE: usize = 3;

/// Most tasks that try one prompt: the first and its `attempt_of`s
/// (`docs/specs/attempts.md`). Each is an agent, a worktree and a branch.
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub(super) id: TaskId,
    pub(super) project: ProjectId,
    /// For one agent in its own copy (`solo`), its one worker: stopping it
    /// ends the task.
    pub(super) coordinator: SessionId,
    pub(super) workers: Vec<Worker>,
    /// One agent in its own copy, without a coordinator
    /// (`docs/specs/more-agents.md`).
    #[serde(default)]
    pub(super) solo: bool,
    /// A solo task trying the prompt of that first solo task again.
    #[serde(default)]
    attempt_of: Option<TaskId>,
    /// Workers that could not start, latest attempt per name.
    #[serde(default)]
    failed: Vec<FailedWorker>,
    pub(super) limits: TaskLimits,
    /// Workers being set up, which count against `max_workers`.
    #[serde(skip)]
    starting: Vec<Starting>,
    /// The person stopped a solo task while its worker was being set up:
    /// it must not start once set up.
    #[serde(skip)]
    stop_asked: bool,
    /// Workers whose worktree was removed, or is being removed, after the
    /// task ended.
    #[serde(default)]
    cleaned: Vec<SessionId>,
    /// Someone stopped the coordinator; only then are worktrees removed.
    #[serde(default)]
    ended: bool,
    /// Its workers' worktrees: ports, copied files, how far prepared.
    #[serde(default)]
    pub(super) prep: Vec<prepare::Prep>,
}

/// A worker being set up.
#[derive(Debug, Clone)]
struct Starting {
    name: String,
    /// It will wait for a place in the project.
    queued: bool,
    /// The first of its ports.
    pub(super) port: u16,
}

impl Entry {
    pub(super) fn has_session(&self, session: &SessionId) -> bool {
        &self.coordinator == session || self.workers.iter().any(|w| &w.session == session)
    }

    /// Workers being set up: how many will wait for a place, how many
    /// will work.
    pub(super) fn starting(&self) -> (usize, usize) {
        let queued = self.starting.iter().filter(|s| s.queued).count();
        (queued, self.starting.len() - queued)
    }

    /// The names of the workers being set up.
    pub(super) fn starting_names(&self) -> impl Iterator<Item = &str> {
        self.starting.iter().map(|s| s.name.as_str())
    }

    /// The first ports of its workers' blocks, those being set up too.
    pub(super) fn ports(&self) -> impl Iterator<Item = u16> + '_ {
        let made = self.prep.iter().map(|p| p.port);
        made.chain(self.starting.iter().map(|s| s.port))
    }

    fn sessions(&self) -> impl Iterator<Item = &SessionId> {
        std::iter::once(&self.coordinator)
            .filter(|_| !self.solo)
            .chain(self.workers.iter().map(|w| &w.session))
    }
}

pub(super) async fn handle(core: &Core, request: OrchestrationRequest) -> Result<Response> {
    match request {
        OrchestrationRequest::Start {
            project,
            prompt,
            limits,
            context,
            agent,
            solo,
            attempt_of,
            model,
            plan_first,
            repos,
        } => {
            if plan_first && !solo {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "plan first is for one agent in the project or in its own copy",
                ));
            }
            if !repos.is_empty() {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "tasks across several repositories are not built yet",
                ));
            }
            if attempt_of.is_some() && !solo {
                return Err(ProtocolError::new(
                    ErrorCode::BadRequest,
                    "only one agent in its own copy can be another attempt",
                ));
            }
            agents::check_model(agent, model)?;
            let sees = agents::sees_images(&core.agents_ctx(), agent);
            let context = crate::context::message(core, &project, None, &context, sees).await?;
            let task = if solo {
                let solo = Solo {
                    agent,
                    attempt_of,
                    model,
                    plan_first,
                    schedule: None,
                };
                start_solo(core, project, prompt, context, solo).await?
            } else {
                start(core, project, prompt, context, limits, agent, model).await?
            };
            Ok(Response::Orchestration(OrchestrationResponse::Task(task)))
        }
        OrchestrationRequest::List { project } => {
            core.project_store().get(&project)?;
            clean_up(core, &project).await;
            let entries: Vec<Entry> = core
                .orchestration_state()
                .lock()
                .tasks
                .iter()
                .filter(|t| t.project == project)
                .cloned()
                .collect();
            let tasks = entries.iter().map(|t| view(core, t)).collect();
            Ok(Response::Orchestration(OrchestrationResponse::List(tasks)))
        }
        OrchestrationRequest::Stop { task } => {
            let entry = find(core, &task)?;
            if entry.solo && !entry.starting.is_empty() {
                // Its worker has no session yet: [`start_solo`] stops it.
                let state = core.orchestration_state();
                let mut guard = state.lock();
                if let Some(t) = guard.tasks.iter_mut().find(|t| t.id == task) {
                    t.stop_asked = true;
                }
                return Ok(Response::Ok(OkResponse::Ok));
            }
            let ctx = core.agents_ctx();
            for session in entry.sessions() {
                // Sessions that already ended are fine to skip.
                let _ = agents::stop(&ctx, session.clone());
            }
            // A coordinator that already ended (it crashed, or Ovra shut
            // down) gets no new status: the person's stop ends its task.
            let ended = core
                .sessions()
                .find(&entry.coordinator, |s| s.status().is_ended())
                .unwrap_or(false);
            if ended {
                set_ended(core, &entry.coordinator, true);
                clean_up(core, &entry.project).await;
            }
            Ok(Response::Ok(OkResponse::Ok))
        }
        OrchestrationRequest::RetrySetup {
            session,
            skip_setup,
        } => {
            // Prepared in the background; its worker's status follows.
            let _launching = prepare::retry(core, &session, skip_setup)?;
            Ok(Response::Ok(OkResponse::Ok))
        }
        OrchestrationRequest::Leftovers { project } => {
            let leftovers = prepare::leftovers(core, &project).await?;
            Ok(Response::Orchestration(OrchestrationResponse::Leftovers(
                leftovers,
            )))
        }
        OrchestrationRequest::RemoveLeftover { project, path } => {
            prepare::remove_leftover(core, &project, &path).await?;
            Ok(Response::Ok(OkResponse::Ok))
        }
    }
}

/// Starts a coordinator in the main checkout. It reads only, so it may run
/// next to a direct session.
async fn start(
    core: &Core,
    project: ProjectId,
    prompt: String,
    context: Option<crate::context::Context>,
    limits: TaskLimits,
    agent: AgentKind,
    model: Option<AgentModel>,
) -> Result<Task> {
    if limits.max_workers == 0 || limits.max_workers > MAX_WORKERS_CAP {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("max_workers must be between 1 and {MAX_WORKERS_CAP}"),
        ));
    }
    // Workers need a snapshot of the checkout; find out now, not when the
    // coordinator's first spawn fails out of sight.
    needs_git(core, &project, "Plan and split").await?;
    let new = NewSession {
        no_web: false,
        model,
        project: project.clone(),
        agent,
        mode: SessionMode::Direct,
        prompt,
        context,
        cwd: None,
        tools: Some(ToolRole::Coordinator),
        from: MessageFrom::Person,
        account: None,
        queued: false,
        preparing: false,
        plan_first: false,
        schedule: None,
    };
    let coordinator = agents::start(&core.agents_ctx(), new).await?.id;
    let entry = {
        let orchestration = core.orchestration_state();
        let mut state = orchestration.lock();
        state.tasks_made += 1;
        let entry = Entry {
            id: TaskId(format!("t{}", state.tasks_made)),
            project: project.clone(),
            coordinator,
            workers: Vec::new(),
            failed: Vec::new(),
            limits,
            starting: Vec::new(),
            stop_asked: false,
            cleaned: Vec::new(),
            ended: false,
            prep: Vec::new(),
            solo: false,
            attempt_of: None,
        };
        state.tasks.push(entry.clone());
        orchestration.save(&state);
        entry
    };
    core.emit(OrchestrationEvent::Changed {
        project,
        task: entry.id.clone(),
    });
    Ok(view(core, &entry))
}

/// Refuses a task in a project that is not a git repository: each of its
/// agents works on its own copy (a worktree).
async fn needs_git(core: &Core, project: &ProjectId, what: &str) -> Result<()> {
    let root = core.root(project)?;
    match core.worktrees().snapshot(root).await {
        Ok(_) => Ok(()),
        Err(err) => Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!(
                "{what} needs a git repository, because the agent works on its own copy of \
                 the project. Choose \"One agent, here\" to work in this folder. ({})",
                err.message
            ),
        )),
    }
}

/// The name of a solo task's one worker, and so of its branch.
const SOLO: &str = "agent";

/// How a solo task's agent works.
pub(crate) struct Solo {
    pub(crate) agent: AgentKind,
    pub(crate) attempt_of: Option<TaskId>,
    pub(crate) model: Option<AgentModel>,
    pub(crate) plan_first: bool,
    /// The schedule that starts it (ADR 0017).
    pub(crate) schedule: Option<String>,
}

/// Starts one agent of `agent`'s provider in its own worktree, without a
/// coordinator: the task's only worker, with the person's prompt. Past the
/// project's cap it waits `queued`, like a person's start.
pub(crate) async fn start_solo(
    core: &Core,
    project: ProjectId,
    prompt: String,
    context: Option<crate::context::Context>,
    solo: Solo,
) -> Result<Task> {
    let Solo {
        agent,
        attempt_of,
        model,
        plan_first,
        schedule,
    } = solo;
    agents::check_text(&prompt)?;
    if plan_first {
        agents::can_plan_first(&core.agents_ctx(), agent)?;
    }
    needs_git(core, &project, "One agent, own copy").await?;
    let state = core.orchestration_state();
    let Some(worktrees_dir) = state.worktrees_dir.clone() else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "an agent in its own copy needs a worktrees folder, which this front end does not set",
        ));
    };
    let limit = slots::limit(core);
    let low = prepare::low_disk(core, &worktrees_dir).await;
    let (entry, queued, port) = {
        let mut guard = state.lock();
        if let Some(first) = &attempt_of {
            attempt_fits(&guard.tasks, &project, first)?;
        }
        let queued = low.is_some()
            || slots::count(core, &guard, &project).0 >= limit
            || crate::resources::holds_back(core);
        let port = prepare::free_port(&guard).ok_or_else(|| {
            conflict("every block of ports for worktrees is taken; finish or clean up agents first")
        })?;
        guard.tasks_made += 1;
        let entry = Entry {
            id: TaskId(format!("t{}", guard.tasks_made)),
            project: project.clone(),
            // Set to its worker once that has a session.
            coordinator: SessionId(String::new()),
            workers: Vec::new(),
            failed: Vec::new(),
            limits: TaskLimits {
                max_workers: 1,
                budget_micro_usd: None,
            },
            starting: vec![Starting {
                name: SOLO.to_owned(),
                queued,
                port,
            }],
            stop_asked: false,
            cleaned: Vec::new(),
            ended: false,
            prep: Vec::new(),
            solo: true,
            attempt_of,
        };
        guard.tasks.push(entry.clone());
        (entry, queued, port)
    };
    let new = NewWorker {
        name: SOLO,
        prompt,
        context,
        agent,
        model,
        from: MessageFrom::Person,
        queued,
        low,
        port,
        skip_setup: false,
        plan_first,
        schedule,
    };
    let result = create_worker(core, &entry, &worktrees_dir, new).await;
    {
        let mut guard = state.lock();
        match &result {
            Ok((worker, _, prep)) => {
                if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == entry.id) {
                    task.starting.clear();
                    task.coordinator = worker.session.clone();
                    task.workers.push(worker.clone());
                    task.prep.push(prep.clone());
                }
            }
            // Nothing started: the person sees why, and no task is left.
            Err(_) => guard.tasks.retain(|t| t.id != entry.id),
        }
        state.save(&guard);
    }
    let (worker, workspace, _) = result?;
    review::set_workspace(core, &worker.session, workspace);
    super::watch::start(core, &worker.session);
    let stop_asked = find(core, &entry.id).is_ok_and(|t| t.stop_asked);
    if stop_asked {
        let _ = agents::stop(&core.agents_ctx(), worker.session.clone());
        set_ended(core, &worker.session, true);
        clean_up(core, &project).await;
    }
    core.emit(OrchestrationEvent::Changed {
        project: project.clone(),
        task: entry.id.clone(),
    });
    if stop_asked {
        return find(core, &entry.id).map(|entry| view(core, &entry));
    }
    if queued {
        slots::start_queued(core, &project);
    } else {
        let launching = {
            let mut guard = state.lock();
            prepare::spawn_launch(core, &mut guard, &worker.session)
        };
        prepare::joined(launching).await?;
    }
    find(core, &entry.id).map(|entry| view(core, &entry))
}

/// Refuses another attempt at `first` unless it is a solo task of
/// `project` that is no attempt itself, with room for one more.
fn attempt_fits(tasks: &[Entry], project: &ProjectId, first: &TaskId) -> Result<()> {
    let Some(entry) = tasks.iter().find(|t| &t.id == first) else {
        return Err(not_found(format!("task {}", first.0)));
    };
    if &entry.project != project || !entry.solo || entry.attempt_of.is_some() {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "another attempt needs the first attempt: one agent in its own copy, in this project",
        ));
    }
    let made = tasks
        .iter()
        .filter(|t| t.attempt_of.as_ref() == Some(first))
        .count();
    if made + 1 >= MAX_ATTEMPTS {
        return Err(conflict(format!(
            "at most {MAX_ATTEMPTS} attempts try one task"
        )));
    }
    Ok(())
}

pub(super) fn find(core: &Core, task: &TaskId) -> Result<Entry> {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .find(|t| &t.id == task)
        .cloned()
        .ok_or_else(|| not_found(format!("task {}", task.0)))
}

/// The task `worker` works for.
pub(super) fn of_worker(core: &Core, worker: &SessionId) -> Option<Entry> {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .find(|t| t.workers.iter().any(|w| &w.session == worker))
        .cloned()
}

/// Tells a worker's coordinator, as its next message, that something
/// happened to the worker it may want to act on, so a coordinator that ended
/// its turn wakes up. Only Ovra's own words: nothing a worker wrote is
/// passed on. A coordinator that ended is not resumed.
pub(super) fn tell_coordinator(core: &Core, worker: &SessionId, what: &str) {
    let task = core
        .orchestration_state()
        .lock()
        .tasks
        .iter()
        .find(|t| t.workers.iter().any(|w| &w.session == worker))
        .cloned();
    let Some(task) = task.filter(|t| !t.solo) else {
        return;
    };
    let Some(name) = task
        .workers
        .iter()
        .find(|w| &w.session == worker)
        .map(|w| w.name.clone())
    else {
        return;
    };
    let ended = core
        .sessions()
        .find(&task.coordinator, |s| s.status().is_ended())
        .unwrap_or(true);
    if ended {
        return;
    }
    // The first line is for the person reading the transcript too; the
    // rest, the coordinator's tool hints, the UI folds away.
    let text = format!(
        "Ovra: Worker {name} {what}.\nFor the coordinator: its agent_id is {}. Check with \
         list_agents or read_output, then go on with the task.",
        worker.0
    );
    // A coordinator that is stopping or gone simply misses the note.
    let _ = agents::send(
        &core.agents_ctx(),
        task.coordinator,
        vec![(text, MessageFrom::Ovra)],
    );
}

/// A stopped coordinator takes its workers with it: the person stopped the
/// task. Their worktrees and reviews stay. A coordinator that failed leaves
/// them running.
pub(super) fn stop_workers_of(core: &Core, coordinator: &SessionId) {
    let Ok(task) = of_coordinator(core, coordinator) else {
        return;
    };
    let ctx = core.agents_ctx();
    for worker in task.workers {
        // Workers that already ended are fine to skip.
        let _ = agents::stop(&ctx, worker.session);
    }
}

/// Whether `coordinator` was asked to stop or has ended. An unknown one is
/// refused later, as no task's coordinator.
fn stopped(core: &Core, coordinator: &SessionId) -> bool {
    core.sessions()
        .find(coordinator, |s| s.stopping() || s.status().is_ended())
        .unwrap_or(false)
}

/// The task `coordinator` coordinates.
pub(super) fn of_coordinator(core: &Core, coordinator: &SessionId) -> Result<Entry> {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .find(|t| &t.coordinator == coordinator)
        .cloned()
        .ok_or_else(|| forbidden("only a task's coordinator may manage workers"))
}

pub(super) fn view(core: &Core, entry: &Entry) -> Task {
    Task {
        id: entry.id.clone(),
        project: entry.project.clone(),
        coordinator: (!entry.solo).then(|| entry.coordinator.clone()),
        workers: entry.workers.clone(),
        failed: entry.failed.clone(),
        limits: entry.limits.clone(),
        spent_micro_usd: spent(core, entry),
        attempt_of: entry.attempt_of.clone(),
    }
}

/// What every session of the task has spent so far.
fn spent(core: &Core, entry: &Entry) -> u64 {
    entry
        .sessions()
        .filter_map(|id| core.sessions().find(id, |s| s.info().usage).ok())
        .filter_map(|usage| usage.cost_micro_usd)
        .fold(0, u64::saturating_add)
}

/// Why the task may not start another worker now, if it may not.
/// Sessions that asked for a review, from the state's review items.
pub(super) fn asked_for_review(reviews: &[review::Entry]) -> Vec<SessionId> {
    reviews
        .iter()
        .filter(|r| r.requested)
        .map(|r| r.session.clone())
        .collect()
}

/// Why `entry` may not start another worker now, if it may not. `asked` is
/// [`asked_for_review`] of the current state.
pub(super) fn spawn_blocked(core: &Core, entry: &Entry, asked: &[SessionId]) -> Option<String> {
    let ended = |id: &SessionId| {
        core.sessions()
            .find(id, |s| s.status().is_ended())
            .unwrap_or(true)
    };
    if ended(&entry.coordinator) {
        return Some("the task has ended".to_owned());
    }
    let max = entry.limits.max_workers;
    let most = usize::try_from(max)
        .unwrap_or(usize::MAX)
        .saturating_mul(SPAWNS_PER_PLACE);
    let started = entry.workers.len() + entry.starting.len();
    if started >= most {
        return Some(format!(
            "the task has started {started} workers, the most it may over its life \
             ({SPAWNS_PER_PLACE} times its limit of {max}); finish with the workers it has"
        ));
    }
    // A worker that asked for review and waits idle for the person does no
    // work, so it does not hold a place; a message wakes it again.
    let waiting = |id: &SessionId| {
        let idle = core
            .sessions()
            .find(id, |s| *s.status() == AgentStatus::Idle)
            .unwrap_or(false);
        idle && asked.contains(id)
    };
    let running = entry
        .workers
        .iter()
        .filter(|w| !ended(&w.session) && !waiting(&w.session))
        .count()
        + entry.starting.len();
    if running >= usize::try_from(max).unwrap_or(usize::MAX) {
        return Some(format!(
            "the task already runs {max} workers, its limit; wait for one to finish or stop one"
        ));
    }
    if let Some(budget) = entry.limits.budget_micro_usd {
        let spent = spent(core, entry);
        if spent >= budget {
            return Some(format!(
                "the task has spent ${}, its budget of ${}",
                dollars(spent),
                dollars(budget)
            ));
        }
    }
    None
}

fn dollars(micro: u64) -> String {
    format!("{}.{:02}", micro / 1_000_000, micro % 1_000_000 / 10_000)
}

/// Whether `name` fits a branch name and the UI (`^[a-z0-9][a-z0-9-]{0,39}$`)
/// and the board can address it.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let lower = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    // The board's `ask` names the person and the coordinator so, and the
    // board shows Ovra's own lines and the person's as "Ovra" and "You".
    let reserved = matches!(name, "person" | "coordinator" | "ovra" | "you");
    !reserved
        && name.len() <= 40
        && chars.next().is_some_and(lower)
        && chars.all(|c| lower(c) || c == '-')
}

/// Starts a worker of `coordinator`'s task in a new worktree of the main
/// checkout's current state, on branch `ovra/<task>/<name>`. While the
/// project has as many agents working as it may, the worker waits `queued`
/// in its worktree and starts when a place frees. Its worktree is prepared
/// first (`prepare`), unless `skip_setup`; a failed setup answers which
/// step failed, and a spawn with the same name then retries it.
pub(super) async fn spawn(
    core: &Core,
    coordinator: &SessionId,
    name: String,
    prompt: String,
    skip_setup: bool,
) -> Result<Worker> {
    if !valid_name(&name) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "a worker's name is 1 to 40 lowercase letters, digits and dashes, starting with a letter or digit, and not person or coordinator",
        ));
    }
    agents::check_text(&prompt)?;
    if stopped(core, coordinator) {
        return Err(conflict("the task is stopping, so it starts no workers"));
    }
    let state = core.orchestration_state();
    let Some(worktrees_dir) = state.worktrees_dir.clone() else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "workers need a worktrees folder, which this front end does not set",
        ));
    };
    let limit = slots::limit(core);
    if let Some(worker) = failed_setup(core, coordinator, &name) {
        if let Some(launching) = prepare::retry(core, &worker.session, skip_setup)? {
            prepare::joined(launching).await?;
        }
        return worker_now(core, &worker.session).ok_or_else(|| not_found("the worker"));
    }
    let low = prepare::low_disk(core, &worktrees_dir).await;
    let (entry, queued, port) = {
        let mut guard = state.lock();
        let asked = asked_for_review(&guard.reviews);
        let project = guard
            .tasks
            .iter()
            .find(|t| &t.coordinator == coordinator)
            .map(|t| t.project.clone());
        // While the computer is short of memory or CPU, it waits too.
        let queued = low.is_some()
            || project.is_some_and(|p| {
                slots::count(core, &guard, &p).0 >= limit || crate::resources::holds_back(core)
            });
        let port = prepare::free_port(&guard).ok_or_else(|| {
            conflict(
                "every block of ports for worktrees is taken; finish or clean up workers first",
            )
        })?;
        let entry = guard
            .tasks
            .iter_mut()
            .find(|t| &t.coordinator == coordinator)
            .ok_or_else(|| forbidden("only a task's coordinator may start workers"))?;
        if let Some(reason) = spawn_blocked(core, entry, &asked) {
            return Err(conflict(reason));
        }
        let taken = entry.workers.iter().any(|w| w.name == name)
            || entry.starting.iter().any(|s| s.name == name);
        if taken {
            return Err(conflict(format!(
                "the task already has a worker named {name}"
            )));
        }
        entry.starting.push(Starting {
            name: name.clone(),
            queued,
            port,
        });
        (entry.clone(), queued, port)
    };
    // Workers ask for the model their coordinator asked for.
    let model = core
        .agents_ctx()
        .sessions
        .find(coordinator, |s| s.info().model)
        .ok()
        .flatten();
    let new = NewWorker {
        name: &name,
        prompt,
        context: None,
        agent: AgentKind::Claude,
        model,
        from: MessageFrom::Coordinator,
        queued,
        low,
        port,
        skip_setup,
        plan_first: false,
        schedule: None,
    };
    let result = create_worker(core, &entry, &worktrees_dir, new).await;
    {
        let mut guard = state.lock();
        if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == entry.id) {
            task.starting.retain(|s| s.name != name);
            task.failed.retain(|f| f.name != name);
            match &result {
                Ok((worker, _, prep)) => {
                    task.workers.push(worker.clone());
                    task.prep.push(prep.clone());
                }
                // The person sees it with the task, not only the coordinator.
                Err(err) => task.failed.push(FailedWorker {
                    name: name.clone(),
                    reason: err.message.clone(),
                }),
            }
        }
        state.save(&guard);
    }
    if result.is_err() {
        core.emit(OrchestrationEvent::Changed {
            project: entry.project.clone(),
            task: entry.id.clone(),
        });
    }
    let (worker, workspace, _) = result?;
    review::set_workspace(core, &worker.session, workspace);
    super::watch::start(core, &worker.session);
    // The person stopped the task while this worker was set up: it must not
    // work on without its coordinator ([`stop_workers_of`] missed it).
    // A coordinator that failed leaves its workers running, this one too.
    let asked_to_stop = core
        .sessions()
        .find(coordinator, |s| {
            s.stopping() || *s.status() == AgentStatus::Stopped
        })
        .unwrap_or(false);
    if asked_to_stop {
        let _ = agents::stop(&core.agents_ctx(), worker.session.clone());
    }
    core.emit(OrchestrationEvent::Changed {
        project: entry.project.clone(),
        task: entry.id.clone(),
    });
    if queued {
        // A place may have freed while it was set up.
        slots::start_queued(core, &entry.project);
        return Ok(worker);
    }
    if asked_to_stop {
        return Ok(worker);
    }
    let launching = {
        let mut guard = state.lock();
        prepare::spawn_launch(core, &mut guard, &worker.session)
    };
    prepare::joined(launching).await?;
    Ok(worker_now(core, &worker.session).unwrap_or(worker))
}

/// The worker of `coordinator`'s task named `name`, if its setup failed.
fn failed_setup(core: &Core, coordinator: &SessionId, name: &str) -> Option<Worker> {
    let state = core.orchestration_state().lock();
    let task = state.tasks.iter().find(|t| &t.coordinator == coordinator)?;
    let worker = task.workers.iter().find(|w| w.name == name)?;
    prepare::failed(&state, &worker.session).then(|| worker.clone())
}

/// The worker `session` as its task has it now.
fn worker_now(core: &Core, session: &SessionId) -> Option<Worker> {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .flat_map(|t| &t.workers)
        .find(|w| &w.session == session)
        .cloned()
}

/// What [`create_worker`] makes.
struct NewWorker<'a> {
    name: &'a str,
    prompt: String,
    /// Ovra's message after the prompt, the person's `@` context, and its
    /// images.
    context: Option<crate::context::Context>,
    agent: AgentKind,
    model: Option<AgentModel>,
    /// Who wrote `prompt`.
    from: MessageFrom,
    queued: bool,
    /// Why its worktree waits for disk space, if it does.
    low: Option<String>,
    port: u16,
    skip_setup: bool,
    plan_first: bool,
    /// The schedule that starts it (ADR 0017).
    schedule: Option<String>,
}

/// A worker's session, waiting `preparing` or `queued`, and its worktree
/// unless that waits for disk space.
async fn create_worker(
    core: &Core,
    task: &Entry,
    worktrees_dir: &std::path::Path,
    new: NewWorker<'_>,
) -> Result<(Worker, Workspace, prepare::Prep)> {
    let NewWorker {
        name,
        prompt,
        context,
        agent,
        model,
        from,
        queued,
        low,
        port,
        skip_setup,
        plan_first,
        schedule,
    } = new;
    let root = core.root(&task.project)?;
    let worktrees = core.worktrees();
    let base = worktrees.snapshot(root.clone()).await?;
    let dir: PathBuf = worktrees_dir
        .join(&task.project.0)
        .join(&task.id.0)
        .join(name);
    let branch = format!("ovra/{}/{name}", task.id.0);
    let reason = format!("Ovra: {}/{name}", task.id.0);
    let made = match low {
        Some(_) => None,
        None => Some(
            prepare::make(
                core,
                &root,
                dir.clone(),
                branch.clone(),
                base.clone(),
                reason,
            )
            .await?,
        ),
    };
    let (path, branch, base) = match &made {
        Some(made) => (made.path.clone(), made.branch.clone(), made.base.clone()),
        None => (dir, branch, base),
    };
    // Before the worker runs: the project's hooks and config at its start.
    let git = worktrees
        .git_files(root.clone())
        .await
        .ok()
        .map(|g| g.files);
    // Workers bill the account their coordinator runs with; a solo task's
    // one worker, like a person's start, the project's.
    let account = (!task.solo).then(|| {
        core.agents_ctx()
            .sessions
            .find(&task.coordinator, |s| s.info().account)
            .ok()
            .flatten()
            .unwrap_or_else(|| crate::protocol::agents::DEFAULT_ACCOUNT.to_owned())
    });
    let new = NewSession {
        no_web: false,
        model,
        project: task.project.clone(),
        agent,
        mode: SessionMode::Isolated,
        prompt,
        context,
        cwd: Some(path.clone()),
        tools: Some(ToolRole::Worker),
        from,
        account,
        queued,
        preparing: !queued,
        plan_first,
        schedule,
    };
    let session = match agents::start(&core.agents_ctx(), new).await {
        Ok(session) => session.id,
        Err(err) => {
            // Nothing ran in it; a worktree left behind would only confuse.
            if let Some(made) = made {
                let _ = remove(core, &root, made.path, made.included).await;
            }
            return Err(err);
        }
    };
    let mut prep = prepare::Prep::new(session.clone(), port, base.clone());
    prep.ready = skip_setup;
    if low.is_some() {
        prep.disk_checked = Some(tokio::time::Instant::now());
    }
    if let Some(made) = made {
        prep.made = true;
        prep.included = made.included;
        prep.note = made.note;
    }
    let worker = Worker {
        repos: Vec::new(),
        session,
        name: name.to_owned(),
        branch,
        worktree: path.to_string_lossy().into_owned(),
        kept: None,
        setup: None,
        disk: None,
        waiting: low,
        included: prepare::shown(&prep.included),
    };
    let workspace = Workspace {
        dir: path,
        base,
        git,
    };
    Ok((worker, workspace, prep))
}

/// Runs [`clean_up`] again once the worker's worktree is idle.
fn retry_when_idle(core: &Core, project: &ProjectId, session: &SessionId) {
    let (Some(idle), Ok(runtime)) = (
        crate::servers::when_idle(core, session),
        tokio::runtime::Handle::try_current(),
    ) else {
        return;
    };
    let (weak, project) = (core.downgrade(), project.clone());
    let again: crate::services::BoxFuture<'static, ()> = Box::pin(async move {
        idle.await;
        if let Some(core) = weak.upgrade() {
            clean_up(&core, &project).await;
        }
    });
    runtime.spawn(again);
}

/// Removes the worktrees of an ended task's workers once nothing in them
/// waits for the person: the worker ended, no item of it is partly approved
/// and its worktree has no change its review has not decided (the decided
/// state is the review's snapshot, which stays kept); then its branch goes
/// too, unless it holds other commits. On any doubt nothing is removed.
pub(super) async fn clean_up(core: &Core, project: &ProjectId) {
    let state = core.orchestration_state();
    let candidates: Vec<(TaskId, Worker, Vec<IncludedFile>)> = state
        .lock()
        .tasks
        .iter()
        .filter(|t| &t.project == project && t.ended)
        .flat_map(|t| {
            t.workers
                .iter()
                .filter(|w| !t.cleaned.contains(&w.session))
                .map(|w| {
                    let prep = t.prep.iter().find(|p| p.session == w.session);
                    let included = prep.map(|p| p.included.clone()).unwrap_or_default();
                    (t.id.clone(), w.clone(), included)
                })
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let Ok(root) = core.root(project) else {
        return;
    };
    for (task, worker, included) in candidates {
        // Marked first, together with the check that it ended: from here
        // on it cannot be resumed (see `unless_cleaned`), so what `settled`
        // finds stays true while the worktree goes.
        if !mark(core, &task, &worker.session) {
            continue;
        }
        let dir = PathBuf::from(&worker.worktree);
        let worktrees = core.worktrees();
        let settled = match review::settled(core, &worker.session).await {
            Some(base) if crate::servers::worktree_idle(core, &worker.session).await => Some(base),
            // A server or pod still holds the worktree: tried again once
            // it ended, without making anyone wait.
            Some(_) => {
                retry_when_idle(core, project, &worker.session);
                None
            }
            None => None,
        };
        let (removed, kept) = match settled {
            None => (false, None),
            Some(base) => match remove(core, &root, dir, included).await {
                Ok(()) => {
                    // Its start is a snapshot and its decisions are kept as
                    // snapshots; a branch with more than that stays.
                    let _ = worktrees
                        .delete_branch(root.clone(), worker.branch.clone(), base)
                        .await;
                    (true, None)
                }
                // Git refused (files only the worktree holds); other
                // errors are retried next time without a reason shown.
                Err(err) if err.code == ErrorCode::Conflict => (false, Some(err.message)),
                Err(_) => (false, None),
            },
        };
        let mut guard = state.lock();
        let mut changed = removed;
        if let Some(entry) = guard.tasks.iter_mut().find(|t| t.id == task) {
            if removed {
                // Its ports are free again.
                entry.prep.retain(|p| p.session != worker.session);
            } else {
                entry.cleaned.retain(|s| s != &worker.session);
            }
            if let Some(w) = entry
                .workers
                .iter_mut()
                .find(|w| w.session == worker.session)
                && w.kept != kept
            {
                w.kept = kept;
                changed = true;
            }
        }
        state.save(&guard);
        drop(guard);
        if changed {
            core.emit(OrchestrationEvent::Changed {
                project: project.clone(),
                task,
            });
        }
    }
}

/// Removes the worktree at `dir`, unlocked just before; locked again if it
/// stays.
pub(super) async fn remove(
    core: &Core,
    root: &std::path::Path,
    dir: PathBuf,
    included: Vec<IncludedFile>,
) -> Result<()> {
    let worktrees = core.worktrees();
    let _ = worktrees.unlock(root.to_owned(), dir.clone()).await;
    let removed = worktrees
        .remove(root.to_owned(), dir.clone(), included)
        .await;
    if removed.is_err() {
        let _ = worktrees
            .lock(root.to_owned(), dir, "Ovra: kept".to_owned())
            .await;
    }
    removed
}

/// Marks `worker` of `task` cleaned if it has ended and is not marked yet.
fn mark(core: &Core, task: &TaskId, worker: &SessionId) -> bool {
    let state = core.orchestration_state();
    let mut guard = state.lock();
    let Some(entry) = guard.tasks.iter_mut().find(|t| &t.id == task) else {
        return false;
    };
    // The sessions' lock is taken inside this one, as the lock order allows.
    let ended = core
        .sessions()
        .find(worker, |s| s.status().is_ended())
        .unwrap_or(false);
    if !ended || entry.cleaned.contains(worker) {
        return false;
    }
    entry.cleaned.push(worker.clone());
    true
}

/// Runs `restart` (a resume of `session`) unless `session` is a worker
/// whose worktree is removed or being removed; checked and run under one
/// lock, so the two never cross.
pub(crate) fn unless_cleaned<T>(
    core: &Core,
    session: &SessionId,
    restart: impl FnOnce() -> T,
) -> Result<T> {
    let state = core.orchestration_state();
    let guard = state.lock();
    if guard.tasks.iter().any(|t| t.cleaned.contains(session)) {
        return Err(conflict(
            "this worker's worktree is removed, or being removed, because its task had ended; start a new worker to rework it",
        ));
    }
    let restarted = restart();
    drop(guard);
    Ok(restarted)
}

/// Keeps whether `coordinator`'s task has ended: someone stopped the
/// coordinator (not a crash, a shutdown or a restart), or it runs again.
pub(super) fn set_ended(core: &Core, coordinator: &SessionId, ended: bool) {
    let state = core.orchestration_state();
    let mut guard = state.lock();
    let Some(entry) = guard
        .tasks
        .iter_mut()
        .find(|t| &t.coordinator == coordinator)
    else {
        return;
    };
    if entry.ended != ended {
        entry.ended = ended;
        let sessions: Vec<SessionId> = entry.sessions().cloned().collect();
        // An ended task's workers change nothing more that overlaps.
        if ended {
            guard.changed.retain(|s, _| !sessions.contains(s));
            guard
                .overlaps
                .retain(|(a, b)| !sessions.contains(a) && !sessions.contains(b));
        }
        state.save(&guard);
    }
}

/// Whether `session` is a worker whose worktree was cleaned up.
pub(super) fn cleaned(core: &Core, session: &SessionId) -> bool {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .any(|t| t.cleaned.contains(session))
}
