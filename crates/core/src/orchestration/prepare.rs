//! A worker's new worktree made ready before its agent starts
//! (`docs/specs/worktree-setup.md`): locked, the files `.worktreeinclude`
//! lists copied in, the steps of `.ovra/worktree.json` run in its pod with
//! a block of ports of its own. Meanwhile the worker is `preparing` and
//! takes a place; a failed step keeps it from starting until the
//! coordinator or the person retries or starts it anyway.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use super::{Result, State, conflict, setup, slots, tasks};
use crate::Core;
use crate::agents::{self, AgentHost};
use crate::core::WeakCore;
use crate::path::RelPath;
use crate::protocol::ErrorCode;
use crate::protocol::agents::{AgentStatus, AgentsEvent, MessageFrom, SessionId};
use crate::protocol::fs::FileBody;
use crate::protocol::orchestration::{OrchestrationEvent, SetupProgress};
use crate::protocol::projects::ProjectId;
use crate::services::{CommandLine, IncludedFile, SetupEnd, SetupSink, SetupStep};
use crate::untrusted::untrusted;

/// Each work dir's ports: `OVRA_PORT` to `OVRA_PORT + 9`.
const PORT_BLOCK: u16 = 10;
/// Where blocks are handed out, first ports from here...
const FIRST_PORT: u16 = 41000;
/// ...to the block that ends here.
const LAST_PORT: u16 = 48999;

/// A new worktree waits while the data folder's volume has less free.
const MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// How often a worker that waits for disk space asks again.
const DISK_RECHECK: Duration = Duration::from_secs(60);

/// The lines of a step's output a worker's status keeps.
const LOG_LINES: usize = 50;

/// A worktree's size is counted at most this often...
const USAGE_EVERY: Duration = Duration::from_secs(60);
/// ...walking for at most this long.
const USAGE_BUDGET: Duration = Duration::from_secs(2);

/// What a worker's worktree needs beyond its [`Worker`](crate::protocol::orchestration::Worker)
/// record, kept with its task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Prep {
    pub(super) session: SessionId,
    /// The first of its [`PORT_BLOCK`] ports; freed with the record.
    pub(super) port: u16,
    /// The snapshot its worktree starts from.
    base: String,
    /// What `.worktreeinclude` copied in, which does not block removal.
    #[serde(default)]
    pub(super) included: Vec<IncludedFile>,
    /// Its worktree exists and is locked.
    pub(super) made: bool,
    /// Its setup succeeded, was skipped or has no steps: the agent may start.
    pub(super) ready: bool,
    /// Ovra's words for its first message: what went wrong before it.
    #[serde(default)]
    pub(super) note: Option<String>,
    /// When its size was last counted.
    #[serde(skip)]
    counted: Option<Instant>,
    /// When the free disk space was last asked for it, and whether that
    /// is being asked now.
    #[serde(skip)]
    pub(super) disk_checked: Option<Instant>,
    #[serde(skip)]
    disk_checking: bool,
}

impl Prep {
    pub(super) fn new(session: SessionId, port: u16, base: String) -> Self {
        Self {
            session,
            port,
            base,
            included: Vec::new(),
            made: false,
            ready: false,
            note: None,
            counted: None,
            disk_checked: None,
            disk_checking: false,
        }
    }
}

/// A worktree made by [`make`].
pub(super) struct Made {
    pub(super) path: PathBuf,
    pub(super) branch: String,
    pub(super) base: String,
    pub(super) included: Vec<IncludedFile>,
    pub(super) note: Option<String>,
}

/// The first free block of ports, or `None` when all are taken.
pub(super) fn free_port(state: &State) -> Option<u16> {
    let taken: std::collections::BTreeSet<u16> =
        state.tasks.iter().flat_map(tasks::Entry::ports).collect();
    (FIRST_PORT..=LAST_PORT - (PORT_BLOCK - 1))
        .step_by(PORT_BLOCK.into())
        .find(|p| !taken.contains(p))
}

/// Why a new worktree in `dir` must wait, if the volume is low; unknown
/// free space waits for nothing.
pub(super) async fn low_disk(core: &Core, dir: &Path) -> Option<String> {
    let free = core.worktrees().free_space(dir.to_owned()).await.ok()?;
    (free < MIN_FREE_BYTES).then(|| {
        format!(
            "waits for disk space: the volume of Ovra's data folder has {} free, and a new \
             worktree needs at least 5 GiB",
            gib(free)
        )
    })
}

fn gib(bytes: u64) -> String {
    let tenths = bytes / (1024 * 1024 * 1024 / 10);
    format!("{}.{} GiB", tenths / 10, tenths % 10)
}

/// Makes the worktree at `dir` on `branch` from `base`, locks it against
/// the person's `git worktree prune` and copies in what `.worktreeinclude`
/// lists.
pub(super) async fn make(
    core: &Core,
    root: &Path,
    dir: PathBuf,
    branch: String,
    base: String,
    reason: String,
) -> Result<Made> {
    let worktrees = core.worktrees();
    let info = worktrees.create(root.to_owned(), dir, branch, base).await?;
    // A worktree that cannot be locked still works; the lock only guards
    // it against the person's own git commands.
    let _ = worktrees
        .lock(root.to_owned(), info.path.clone(), reason)
        .await;
    let (included, note) = match worktrees.include(root.to_owned(), info.path.clone()).await {
        Ok(included) => (included.copied, None),
        Err(err) if err.code == ErrorCode::Unavailable => (Vec::new(), None),
        Err(err) => (
            Vec::new(),
            Some(format!(
                "Ovra: the files .worktreeinclude lists could not be copied into this worktree \
                 ({}).",
                err.message
            )),
        ),
    };
    Ok(Made {
        path: info.path,
        branch: info.branch,
        base: info.base,
        included,
        note,
    })
}

/// The worker `session`'s task, worker and record, changed by `f` under
/// the lock and saved; `None` if it is not a worker with a record.
fn with_prep<T>(
    core: &Core,
    session: &SessionId,
    f: impl FnOnce(&mut tasks::Entry, usize, usize) -> T,
) -> Option<T> {
    let state = core.orchestration_state();
    let mut guard = state.lock();
    let task = guard.tasks.iter_mut().find(|t| t.has_session(session))?;
    let worker = task.workers.iter().position(|w| &w.session == session)?;
    let prep = task.prep.iter().position(|p| &p.session == session)?;
    let out = f(task, worker, prep);
    state.save(&guard);
    Some(out)
}

fn changed(core: &Core, project: &ProjectId, task: &crate::protocol::orchestration::TaskId) {
    core.emit(OrchestrationEvent::Changed {
        project: project.clone(),
        task: task.clone(),
    });
}

/// Whether `session` is a worker whose setup failed and waits for a retry.
pub(super) fn failed(state: &State, session: &SessionId) -> bool {
    state
        .tasks
        .iter()
        .flat_map(|t| &t.workers)
        .any(|w| &w.session == session && w.setup.as_ref().is_some_and(|s| s.failed.is_some()))
}

/// Whether the queued worker `session` still has its worktree to prepare.
pub(super) fn pending(state: &State, session: &SessionId) -> Option<bool> {
    let prep = state
        .tasks
        .iter()
        .flat_map(|t| &t.prep)
        .find(|p| &p.session == session)?;
    Some(!(prep.made && prep.ready))
}

/// Whether the queued worker `session` waits for disk space; asks again
/// in the background when it is time.
pub(super) fn waits_for_disk(core: &Core, state: &mut State, session: &SessionId) -> bool {
    let Some(task) = state.tasks.iter_mut().find(|t| t.has_session(session)) else {
        return false;
    };
    if !task
        .workers
        .iter()
        .any(|w| &w.session == session && w.waiting.is_some())
    {
        return false;
    }
    let Some(prep) = task.prep.iter_mut().find(|p| &p.session == session) else {
        return false;
    };
    let due = prep
        .disk_checked
        .is_none_or(|at| at.elapsed() >= DISK_RECHECK);
    if due && !prep.disk_checking {
        prep.disk_checking = true;
        let (core, session) = (core.clone(), session.clone());
        tokio::spawn(async move { recheck_disk(&core, &session).await });
    }
    true
}

async fn recheck_disk(core: &Core, session: &SessionId) {
    let Some(dir) = core.orchestration_state().worktrees_dir.clone() else {
        return;
    };
    let low = low_disk(core, &dir).await;
    let done = with_prep(core, session, |task, w, p| {
        task.prep[p].disk_checking = false;
        task.prep[p].disk_checked = Some(Instant::now());
        let cleared = low.is_none() && task.workers[w].waiting.take().is_some();
        if let Some(low) = &low {
            task.workers[w].waiting = Some(low.clone());
        }
        (task.project.clone(), task.id.clone(), cleared)
    });
    if let Some((project, task, true)) = done {
        changed(core, &project, &task);
        slots::start_queued(core, &project);
    }
}

/// Moves the waiting worker `session` to `preparing` and prepares it in
/// the background; the handle ends with [`launch`]'s result.
pub(super) fn begin(core: &Core, state: &mut State, session: &SessionId) -> Option<Launching> {
    let sessions = core.sessions();
    let project = sessions.find(session, |s| s.info().project).ok()?;
    if !sessions.wait(session, AgentStatus::Preparing) {
        return None;
    }
    core.emit(AgentsEvent::Status {
        session: session.clone(),
        project,
        status: AgentStatus::Preparing,
    });
    Some(spawn_launch(core, state, session))
}

/// A worker being prepared: [`launch`] in a task of its own, so that it
/// goes on if whoever waits for it goes away; a stop aborts it.
pub(super) type Launching = tokio::task::JoinHandle<Result<()>>;

/// Prepares the `preparing` worker `session` in a task of its own.
pub(super) fn spawn_launch(core: &Core, state: &mut State, session: &SessionId) -> Launching {
    let (task_core, id) = (core.clone(), session.clone());
    let handle = tokio::spawn(async move {
        let result = launch(&task_core, &id).await;
        task_core.orchestration_state().lock().setups.remove(&id);
        result
    });
    state.setups.insert(session.clone(), handle.abort_handle());
    handle
}

/// What a [`Launching`] ended with.
pub(super) async fn joined(handle: Launching) -> Result<()> {
    handle
        .await
        .unwrap_or_else(|_| Err(conflict("the worker was stopped while it was prepared")))
}

/// Stops preparing `session`, which ended.
pub(super) fn abort(core: &Core, session: &SessionId) {
    if let Some(handle) = core.orchestration_state().lock().setups.remove(session) {
        handle.abort();
    }
}

/// Makes the `preparing` worker's worktree ready, then starts its agent;
/// a failed setup step leaves it `preparing` with the failure in its
/// status and answers which step failed, with the end of its log.
async fn launch(core: &Core, session: &SessionId) -> Result<()> {
    let Some((project, prep, worker)) = with_prep(core, session, |t, w, p| {
        (t.project.clone(), t.prep[p].clone(), t.workers[w].clone())
    }) else {
        return Err(conflict("this worker is not known"));
    };
    let root = core.root(&project)?;
    let dir = PathBuf::from(&worker.worktree);
    if !prep.made {
        let reason = format!("Ovra: {}", worker.branch.trim_start_matches("ovra/"));
        let made = make(
            core,
            &root,
            dir.clone(),
            worker.branch.clone(),
            prep.base.clone(),
            reason,
        )
        .await;
        let made = match made {
            Ok(made) => made,
            Err(err) => {
                let host = AgentHost::new(
                    session.clone(),
                    project,
                    core.sessions().clone(),
                    core.sink(),
                );
                host.status(AgentStatus::Failed {
                    message: format!("its worktree could not be made: {}", err.message),
                });
                return Err(err);
            }
        };
        with_prep(core, session, |t, _, p| {
            let prep = &mut t.prep[p];
            prep.made = true;
            prep.included = made.included;
            prep.note = made.note;
        });
    }
    if !prep.ready {
        run_setup(core, &project, session, &root, &dir).await?;
    }
    start(core, session).await
}

/// Reads the worktree's `.ovra/worktree.json` and runs its steps. A
/// broken file runs nothing and is said in the first message instead.
async fn run_setup(
    core: &Core,
    project: &ProjectId,
    session: &SessionId,
    root: &Path,
    dir: &Path,
) -> Result<()> {
    let steps = match read(core, dir).await {
        Ok(steps) => steps,
        Err(why) => {
            let note = format!("Ovra: this worktree was not set up, because {why}");
            with_prep(core, session, |t, _, p| {
                let prep = &mut t.prep[p];
                prep.ready = true;
                prep.note = Some(
                    prep.note
                        .take()
                        .map_or(note.clone(), |n| format!("{n}\n{note}")),
                );
            });
            return Ok(());
        }
    };
    if steps.is_empty() {
        with_prep(core, session, |t, _, p| t.prep[p].ready = true);
        return Ok(());
    }
    let port = with_prep(core, session, |t, _, p| t.prep[p].port).unwrap_or(FIRST_PORT);
    let commands: Vec<String> = steps.iter().map(|s| s.command.join(" ")).collect();
    let lines = steps
        .into_iter()
        .map(|def| SetupStep {
            line: step_line(&def.command, dir, root, port),
            timeout: def.timeout,
        })
        .collect();
    let sink = Arc::new(Progress {
        core: core.downgrade(),
        session: session.clone(),
        commands: commands.clone(),
        log: Mutex::default(),
    });
    let end =
        crate::servers::run_steps(core, project, session, dir.to_owned(), lines, sink.clone())
            .await;
    let why = match end {
        Ok(SetupEnd::Done) => None,
        Ok(SetupEnd::Failed { step, code, reason }) => {
            let why = match (reason, code) {
                (Some(reason), _) => format!("could not start: {reason}"),
                (None, Some(code)) => format!("exited with code {code}"),
                (None, None) => "was ended by a signal".to_owned(),
            };
            Some((step, why))
        }
        Ok(SetupEnd::TimedOut { step }) => Some((step, "ran longer than its timeout".to_owned())),
        Err(err) => Some((0, format!("could not run: {}", err.message))),
    };
    let Some((step, why)) = why else {
        let done = with_prep(core, session, |t, w, p| {
            t.prep[p].ready = true;
            t.workers[w].setup = None;
            (t.project.clone(), t.id.clone())
        });
        if let Some((project, task)) = done {
            changed(core, &project, &task);
        }
        return Ok(());
    };
    let log = std::mem::take(&mut *sink.lock_log());
    let step = step.min(commands.len().saturating_sub(1));
    let progress = SetupProgress {
        step: u32::try_from(step).unwrap_or(u32::MAX),
        steps: u32::try_from(commands.len()).unwrap_or(u32::MAX),
        command: commands[step].clone(),
        log: log.clone(),
        failed: Some(why.clone()),
    };
    let found = with_prep(core, session, |t, w, _| {
        t.workers[w].setup = Some(progress);
        (t.project.clone(), t.id.clone(), t.workers[w].name.clone())
    });
    let Some((project, task, name)) = found else {
        return Err(conflict("this worker is not known"));
    };
    changed(core, &project, &task);
    let failure = format!(
        "could not be set up: step {} of {} ({}) {why}",
        step + 1,
        commands.len(),
        commands[step]
    );
    let output = untrusted("setup", &log.join("\n"));
    let next = "Call spawn_agent with its name again to retry the setup, or also with \
                skip_setup to start it without, or tell the person.";
    tasks::tell_coordinator(
        core,
        session,
        &format!("{failure}; its output ended with {output}\n{next}"),
    );
    Err(conflict(format!(
        "worker {name} (agent_id {}) did not start: its worktree {failure}. The last lines of \
         its output:\n{output}\n{next}",
        session.0
    )))
}

/// The worktree's setup steps, or why the file cannot be used.
async fn read(core: &Core, dir: &Path) -> std::result::Result<Vec<setup::SetupDef>, String> {
    let path = RelPath::parse(setup::FILE).map_err(|e| e.message)?;
    match core.fs_service().read_file(dir.to_owned(), path).await {
        Ok(file) => match file.body {
            FileBody::Text { text } => setup::parse(&text),
            _ => Err(format!("{} is not a text file", setup::FILE)),
        },
        Err(e) if e.code == ErrorCode::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{} could not be read: {}", setup::FILE, e.message)),
    }
}

/// A setup step's command line: no shell, in the worktree, with the
/// roots and the first of its ports.
fn step_line(command: &[String], dir: &Path, root: &Path, port: u16) -> CommandLine {
    let var = |k: &str, v: OsString| (OsString::from(k), Some(v));
    CommandLine {
        program: PathBuf::from(&command[0]),
        args: command[1..].iter().map(OsString::from).collect(),
        env: vec![
            var("OVRA_WORKTREE_ROOT", dir.as_os_str().to_owned()),
            var("OVRA_MAIN_ROOT", root.as_os_str().to_owned()),
            var("OVRA_PORT", port.to_string().into()),
        ],
        cwd: dir.to_owned(),
    }
}

/// Puts a running setup's step and log in its worker's status.
#[derive(Debug)]
struct Progress {
    core: WeakCore,
    session: SessionId,
    commands: Vec<String>,
    log: Mutex<Vec<String>>,
}

impl Progress {
    fn lock_log(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn show(&self, index: usize) {
        let Some(core) = self.core.upgrade() else {
            return;
        };
        let progress = SetupProgress {
            step: u32::try_from(index).unwrap_or(u32::MAX),
            steps: u32::try_from(self.commands.len()).unwrap_or(u32::MAX),
            command: self.commands.get(index).cloned().unwrap_or_default(),
            log: self.lock_log().clone(),
            failed: None,
        };
        let shown = with_prep(&core, &self.session, |t, w, _| {
            t.workers[w].setup = Some(progress);
            (t.project.clone(), t.id.clone())
        });
        if let Some((project, task)) = shown {
            changed(&core, &project, &task);
        }
    }
}

impl SetupSink for Progress {
    fn step(&self, index: usize) {
        self.lock_log().clear();
        self.show(index);
    }

    fn output(&self, index: usize, lines: Vec<String>) {
        {
            let mut log = self.lock_log();
            log.extend(lines);
            let extra = log.len().saturating_sub(LOG_LINES);
            log.drain(..extra);
        }
        self.show(index);
    }
}

/// Starts the agent of the prepared worker `session`, with Ovra's note
/// first if something went wrong before.
async fn start(core: &Core, session: &SessionId) -> Result<()> {
    let note = with_prep(core, session, |t, _, p| t.prep[p].note.take()).flatten();
    let Some(mut parts) = core.sessions().take_waiting(session) else {
        return Err(conflict("the worker was stopped while it was prepared"));
    };
    if let Some(note) = note {
        parts.insert(0, (note, MessageFrom::Ovra));
    }
    agents::start_queued(&core.agents_ctx(), session.clone(), parts).await?;
    count_disk(core, session);
    Ok(())
}

/// The person's Retry, or with `skip` their Start anyway, of a worker
/// whose setup failed; the coordinator's is `spawn_agent` again. Prepares
/// it again now if there is room, else it waits `queued`. `Some` while
/// it is prepared.
pub(super) fn retry(core: &Core, session: &SessionId, skip: bool) -> Result<Option<Launching>> {
    let limit = slots::limit(core);
    let state = core.orchestration_state();
    let mut guard = state.lock();
    if !failed(&guard, session) {
        return Err(conflict(
            "only a worker whose setup failed can retry it or start without it",
        ));
    }
    let project = core.sessions().find(session, |s| s.info().project)?;
    let room = slots::count(core, &guard, &project).0 < limit;
    for task in &mut guard.tasks {
        for worker in task.workers.iter_mut().filter(|w| &w.session == session) {
            worker.setup = None;
        }
        for prep in task.prep.iter_mut().filter(|p| &p.session == session) {
            prep.ready |= skip;
        }
    }
    let task = guard
        .tasks
        .iter()
        .find(|t| t.has_session(session))
        .map(|t| t.id.clone());
    state.save(&guard);
    let launching = if room {
        Some(spawn_launch(core, &mut guard, session))
    } else {
        core.sessions().wait(session, AgentStatus::Queued);
        core.emit(AgentsEvent::Status {
            session: session.clone(),
            project: project.clone(),
            status: AgentStatus::Queued,
        });
        None
    };
    drop(guard);
    if let Some(task) = task {
        changed(core, &project, &task);
    }
    Ok(launching)
}

/// Counts the worker's worktree in the background, unless it was counted
/// within the last minute.
pub(super) fn count_disk(core: &Core, session: &SessionId) {
    let due = with_prep(core, session, |t, w, p| {
        let prep = &mut t.prep[p];
        let due = prep.made && prep.counted.is_none_or(|at| at.elapsed() >= USAGE_EVERY);
        if due {
            prep.counted = Some(Instant::now());
        }
        due.then(|| PathBuf::from(&t.workers[w].worktree))
    })
    .flatten();
    let (Some(dir), Ok(runtime)) = (due, tokio::runtime::Handle::try_current()) else {
        return;
    };
    let (core, session) = (core.clone(), session.clone());
    runtime.spawn(async move {
        let Ok(disk) = core.worktrees().usage(dir, USAGE_BUDGET).await else {
            return;
        };
        let shown = with_prep(&core, &session, |t, w, _| {
            let worker = &mut t.workers[w];
            let new = worker.disk != Some(disk);
            worker.disk = Some(disk);
            new.then(|| (t.project.clone(), t.id.clone()))
        })
        .flatten();
        if let Some((project, task)) = shown {
            changed(&core, &project, &task);
        }
    });
}

#[cfg(test)]
mod tests;
