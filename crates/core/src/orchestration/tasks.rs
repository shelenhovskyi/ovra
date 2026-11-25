//! Tasks: one coordinator and the workers it starts, within limits.

use std::path::PathBuf;

use super::{Result, Workspace, conflict, forbidden, not_found, review, slots};
use crate::Core;
use crate::agents::{self, NewSession};
use crate::protocol::agents::{AgentKind, AgentStatus, MessageFrom, SessionId, SessionMode};
use crate::protocol::orchestration::{
    FailedWorker, OrchestrationEvent, OrchestrationRequest, OrchestrationResponse, Task, TaskId,
    TaskLimits, Worker,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::ToolRole;

/// Most workers a task may run at once, whatever it asks for: each is a
/// `claude` process with its own worktree.
const MAX_WORKERS_CAP: u32 = 16;

/// A task may start at most this many times `max_workers` workers over its
/// life: each one is a branch and a worktree, and spawns need no approval.
const SPAWNS_PER_PLACE: usize = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub(super) id: TaskId,
    pub(super) project: ProjectId,
    pub(super) coordinator: SessionId,
    pub(super) workers: Vec<Worker>,
    /// Workers that could not start, latest attempt per name.
    #[serde(default)]
    failed: Vec<FailedWorker>,
    pub(super) limits: TaskLimits,
    /// Workers being set up, which count against `max_workers`.
    #[serde(skip)]
    starting: Vec<Starting>,
    /// Workers whose worktree was removed, or is being removed, after the
    /// task ended.
    #[serde(default)]
    cleaned: Vec<SessionId>,
    /// Someone stopped the coordinator; only then are worktrees removed.
    #[serde(default)]
    ended: bool,
}

/// A worker being set up.
#[derive(Debug, Clone)]
struct Starting {
    name: String,
    /// It will wait for a place in the project.
    queued: bool,
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

    fn sessions(&self) -> impl Iterator<Item = &SessionId> {
        std::iter::once(&self.coordinator).chain(self.workers.iter().map(|w| &w.session))
    }
}

pub(super) async fn handle(core: &Core, request: OrchestrationRequest) -> Result<Response> {
    match request {
        OrchestrationRequest::Start {
            project,
            prompt,
            limits,
        } => {
            let task = start(core, project, prompt, limits).await?;
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
    }
}

/// Starts a coordinator in the main checkout. It reads only, so it may run
/// next to a direct session.
async fn start(
    core: &Core,
    project: ProjectId,
    prompt: String,
    limits: TaskLimits,
) -> Result<Task> {
    if limits.max_workers == 0 || limits.max_workers > MAX_WORKERS_CAP {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!("max_workers must be between 1 and {MAX_WORKERS_CAP}"),
        ));
    }
    // Workers need a snapshot of the checkout; find out now, not when the
    // coordinator's first spawn fails out of sight.
    let root = core.root(&project)?;
    if let Err(err) = core.worktrees().snapshot(root).await {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            format!(
                "Plan and split needs a git repository, because every agent works on its own copy \
                 of the project. Choose \"One agent, here\" to work in this folder. ({})",
                err.message
            ),
        ));
    }
    let new = NewSession {
        project: project.clone(),
        agent: AgentKind::Claude,
        mode: SessionMode::Direct,
        prompt,
        cwd: None,
        tools: Some(ToolRole::Coordinator),
        from: MessageFrom::Person,
        account: None,
        queued: false,
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
            cleaned: Vec::new(),
            ended: false,
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

fn find(core: &Core, task: &TaskId) -> Result<Entry> {
    core.orchestration_state()
        .lock()
        .tasks
        .iter()
        .find(|t| &t.id == task)
        .cloned()
        .ok_or_else(|| not_found(format!("task {}", task.0)))
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
    let Some(task) = task else {
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
        coordinator: entry.coordinator.clone(),
        workers: entry.workers.clone(),
        failed: entry.failed.clone(),
        limits: entry.limits.clone(),
        spent_micro_usd: spent(core, entry),
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

/// Whether `name` fits a branch name and the UI: `^[a-z0-9][a-z0-9-]{0,39}$`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let lower = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    name.len() <= 40 && chars.next().is_some_and(lower) && chars.all(|c| lower(c) || c == '-')
}

/// Starts a worker of `coordinator`'s task in a new worktree of the main
/// checkout's current state, on branch `ovra/<task>/<name>`. While the
/// project has as many agents working as it may, the worker waits `queued`
/// in its worktree and starts when a place frees.
pub(super) async fn spawn(
    core: &Core,
    coordinator: &SessionId,
    name: String,
    prompt: String,
) -> Result<Worker> {
    if !valid_name(&name) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "a worker's name is 1 to 40 lowercase letters, digits and dashes, starting with a letter or digit",
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
    let (entry, queued) = {
        let mut guard = state.lock();
        let asked = asked_for_review(&guard.reviews);
        let project = guard
            .tasks
            .iter()
            .find(|t| &t.coordinator == coordinator)
            .map(|t| t.project.clone());
        let queued =
            project.is_some_and(|p| slots::count(core, &guard, &p).0 >= slots::MAX_WORKING);
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
        });
        (entry.clone(), queued)
    };
    let result = create_worker(core, &entry, &worktrees_dir, &name, prompt, queued).await;
    let mut guard = state.lock();
    if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == entry.id) {
        task.starting.retain(|s| s.name != name);
        task.failed.retain(|f| f.name != name);
        match &result {
            Ok((worker, _)) => task.workers.push(worker.clone()),
            // The person sees it with the task, not only the coordinator.
            Err(err) => task.failed.push(FailedWorker {
                name: name.clone(),
                reason: err.message.clone(),
            }),
        }
    }
    state.save(&guard);
    drop(guard);
    if result.is_err() {
        core.emit(OrchestrationEvent::Changed {
            project: entry.project.clone(),
            task: entry.id.clone(),
        });
    }
    let (worker, workspace) = result?;
    review::set_workspace(core, &worker.session, workspace);
    // The person stopped the task while this worker was set up: it must not
    // work on without its coordinator ([`stop_workers_of`] missed it).
    if stopped(core, coordinator) {
        let _ = agents::stop(&core.agents_ctx(), worker.session.clone());
    }
    core.emit(OrchestrationEvent::Changed {
        project: entry.project.clone(),
        task: entry.id.clone(),
    });
    if queued {
        // A place may have freed while it was set up.
        slots::start_queued(core, &entry.project);
    }
    Ok(worker)
}

async fn create_worker(
    core: &Core,
    task: &Entry,
    worktrees_dir: &std::path::Path,
    name: &str,
    prompt: String,
    queued: bool,
) -> Result<(Worker, Workspace)> {
    let root = core.root(&task.project)?;
    let worktrees = core.worktrees();
    let base = worktrees.snapshot(root.clone()).await?;
    let dir: PathBuf = worktrees_dir
        .join(&task.project.0)
        .join(&task.id.0)
        .join(name);
    let branch = format!("ovra/{}/{name}", task.id.0);
    let info = worktrees.create(root.clone(), dir, branch, base).await?;
    // Before the worker runs: the project's hooks and config at its start.
    let git = worktrees
        .git_files(root.clone())
        .await
        .ok()
        .map(|g| g.files);
    // Workers bill the account their coordinator runs with.
    let account = core
        .agents_ctx()
        .sessions
        .find(&task.coordinator, |s| s.info().account)
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::protocol::agents::DEFAULT_ACCOUNT.to_owned());
    let new = NewSession {
        project: task.project.clone(),
        agent: AgentKind::Claude,
        mode: SessionMode::Isolated,
        prompt,
        cwd: Some(info.path.clone()),
        tools: Some(ToolRole::Worker),
        from: MessageFrom::Coordinator,
        account: Some(account),
        queued,
    };
    let session = match agents::start(&core.agents_ctx(), new).await {
        Ok(session) => session.id,
        Err(err) => {
            // Nothing ran in it; a worktree left behind would only confuse.
            let _ = worktrees.remove(root, info.path).await;
            return Err(err);
        }
    };
    let worker = Worker {
        session,
        name: name.to_owned(),
        branch: info.branch,
        worktree: info.path.to_string_lossy().into_owned(),
        kept: None,
    };
    let workspace = Workspace {
        dir: info.path,
        base: info.base,
        git,
    };
    Ok((worker, workspace))
}

/// Removes the worktrees of an ended task's workers once nothing in them
/// waits for the person: the worker ended, no item of it is partly approved
/// and its worktree has no change its review has not decided (the decided
/// state is the review's snapshot, which stays kept); then its branch goes
/// too, unless it holds other commits. On any doubt nothing is removed.
pub(super) async fn clean_up(core: &Core, project: &ProjectId) {
    let state = core.orchestration_state();
    let candidates: Vec<(TaskId, Worker)> = state
        .lock()
        .tasks
        .iter()
        .filter(|t| &t.project == project && t.ended)
        .flat_map(|t| {
            t.workers
                .iter()
                .filter(|w| !t.cleaned.contains(&w.session))
                .map(|w| (t.id.clone(), w.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let Ok(root) = core.root(project) else {
        return;
    };
    for (task, worker) in candidates {
        // Marked first, together with the check that it ended: from here
        // on it cannot be resumed (see `unless_cleaned`), so what `settled`
        // finds stays true while the worktree goes.
        if !mark(core, &task, &worker.session) {
            continue;
        }
        let dir = PathBuf::from(&worker.worktree);
        let worktrees = core.worktrees();
        let (removed, kept) = match review::settled(core, &worker.session).await {
            None => (false, None),
            Some(base) => match worktrees.remove(root.clone(), dir).await {
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
            if !removed {
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
