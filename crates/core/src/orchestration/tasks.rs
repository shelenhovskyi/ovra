//! Tasks: one coordinator and the workers it starts, within limits.

use std::path::PathBuf;

use super::{Result, Workspace, conflict, forbidden, not_found, review};
use crate::Core;
use crate::agents::{self, NewSession};
use crate::protocol::agents::{AgentKind, SessionId, SessionMode};
use crate::protocol::orchestration::{
    OrchestrationEvent, OrchestrationRequest, OrchestrationResponse, Task, TaskId, TaskLimits,
    Worker,
};
use crate::protocol::projects::ProjectId;
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::ToolRole;

/// Most workers a task may run at once, whatever it asks for: each is a
/// `claude` process with its own worktree.
const MAX_WORKERS_CAP: u32 = 16;

#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) id: TaskId,
    pub(super) project: ProjectId,
    pub(super) coordinator: SessionId,
    pub(super) workers: Vec<Worker>,
    pub(super) limits: TaskLimits,
    /// Names of workers being set up, which count against `max_workers`.
    starting: Vec<String>,
}

impl Entry {
    pub(super) fn has_session(&self, session: &SessionId) -> bool {
        &self.coordinator == session || self.workers.iter().any(|w| &w.session == session)
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
    let new = NewSession {
        project: project.clone(),
        agent: AgentKind::Claude,
        mode: SessionMode::Direct,
        prompt,
        cwd: None,
        tools: Some(ToolRole::Coordinator),
    };
    let coordinator = agents::start(&core.agents_ctx(), new).await?.id;
    let entry = {
        let mut state = core.orchestration_state().lock();
        state.tasks_made += 1;
        let entry = Entry {
            id: TaskId(format!("t{}", state.tasks_made)),
            project: project.clone(),
            coordinator,
            workers: Vec::new(),
            limits,
            starting: Vec::new(),
        };
        state.tasks.push(entry.clone());
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
pub(super) fn spawn_blocked(core: &Core, entry: &Entry) -> Option<String> {
    let ended = |id: &SessionId| {
        core.sessions()
            .find(id, |s| s.status().is_ended())
            .unwrap_or(true)
    };
    if ended(&entry.coordinator) {
        return Some("the task has ended".to_owned());
    }
    let running =
        entry.workers.iter().filter(|w| !ended(&w.session)).count() + entry.starting.len();
    let max = entry.limits.max_workers;
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
/// checkout's current state, on branch `ovra/<task>/<name>`.
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
    let state = core.orchestration_state();
    let Some(worktrees_dir) = state.worktrees_dir.clone() else {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "workers need a worktrees folder, which this front end does not set",
        ));
    };
    let entry = {
        let mut guard = state.lock();
        let entry = guard
            .tasks
            .iter_mut()
            .find(|t| &t.coordinator == coordinator)
            .ok_or_else(|| forbidden("only a task's coordinator may start workers"))?;
        if let Some(reason) = spawn_blocked(core, entry) {
            return Err(conflict(reason));
        }
        if entry.workers.iter().any(|w| w.name == name) || entry.starting.contains(&name) {
            return Err(conflict(format!(
                "the task already has a worker named {name}"
            )));
        }
        entry.starting.push(name.clone());
        entry.clone()
    };
    let result = create_worker(core, &entry, &worktrees_dir, &name, prompt).await;
    let mut guard = state.lock();
    if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == entry.id) {
        task.starting.retain(|n| n != &name);
        if let Ok((worker, _)) = &result {
            task.workers.push(worker.clone());
        }
    }
    let (worker, workspace) = result?;
    drop(guard);
    review::set_workspace(core, &worker.session, workspace);
    core.emit(OrchestrationEvent::Changed {
        project: entry.project.clone(),
        task: entry.id.clone(),
    });
    Ok(worker)
}

async fn create_worker(
    core: &Core,
    task: &Entry,
    worktrees_dir: &std::path::Path,
    name: &str,
    prompt: String,
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
    let new = NewSession {
        project: task.project.clone(),
        agent: AgentKind::Claude,
        mode: SessionMode::Isolated,
        prompt,
        cwd: Some(info.path.clone()),
        tools: Some(ToolRole::Worker),
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
    };
    let workspace = Workspace {
        dir: info.path,
        base: info.base,
    };
    Ok((worker, workspace))
}
