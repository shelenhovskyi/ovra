//! The project's check before review (`docs/specs/review-check.md`): when a
//! worker asks for review, or a person's own agent ends a turn with changes
//! the check has not judged, the command in `.ovra/check.json` of the main
//! checkout runs once in the agent's workspace, in that workspace's pod
//! ([`crate::servers::run_steps`]), so it gets no more than the agent has.
//! `.ovra` is protected, so only the person defines what runs. A finished
//! result is saved with the review item, with the files it judged; its log
//! stays in memory.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::review::{self, Entry};
use crate::Core;
use crate::core::WeakCore;
use crate::path::RelPath;
use crate::protocol::agents::{AgentStatus, SessionId, SessionMode};
use crate::protocol::fs::FileBody;
use crate::protocol::review::{
    ChangedBy, ReviewCheck, ReviewCheckState, ReviewEvent, ReviewId, ReviewState,
};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{CommandLine, FsService, SetupEnd, SetupSink, SetupStep};

/// Where a project defines its check.
pub(super) const FILE: &str = ".ovra/check.json";

/// The time limit when the file gives none.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// The longest a check may be given.
const MAX_TIMEOUT: Duration = Duration::from_secs(3600);

/// The log keeps this many of the last lines...
const MAX_LINES: usize = 2000;

/// ...each cut to this many characters.
const MAX_LINE: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Def {
    command: Vec<String>,
    timeout: Duration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    command: Vec<String>,
    /// Seconds.
    timeout: Option<u64>,
}

/// A finished check, saved with its review item.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Saved {
    check: ReviewCheck,
    /// The look of the workspace it judged, if it could be read.
    tree: Option<String>,
}

/// One run, kept per session: only its latest request counts.
#[derive(Debug)]
pub(super) struct Run {
    review: ReviewId,
    generation: u64,
    command: String,
    state: ReviewCheckState,
    detail: Option<String>,
    log: VecDeque<String>,
    /// Asked for during the worker's turn: it runs once the turn ends, so
    /// it judges the files the worker finished with.
    pending: Option<(Def, Entry)>,
    task: Option<tokio::task::AbortHandle>,
}

impl Run {
    /// Waiting for its turn to end, or running.
    fn unfinished(&self) -> bool {
        self.state == ReviewCheckState::Running
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // Dropping the run's future kills the running command.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Parses and checks the file's text, or says in words why it cannot be
/// used.
fn parse(text: &str) -> Result<Def, String> {
    let file: File = serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
    let at = |why: &str| format!("{FILE}: {why}");
    match file.command.first() {
        None => return Err(at("the command is empty")),
        Some(program) if program.is_empty() => return Err(at("the program is empty")),
        Some(_) if file.command.iter().any(|a| a.contains('\0')) => {
            return Err(at("the command contains a NUL byte"));
        }
        Some(_) => {}
    }
    let timeout = match file.timeout {
        None => DEFAULT_TIMEOUT,
        Some(0) => return Err(at("the timeout must be at least 1 second")),
        Some(secs) if secs > MAX_TIMEOUT.as_secs() => {
            return Err(at(&format!(
                "the timeout must be at most {} seconds",
                MAX_TIMEOUT.as_secs()
            )));
        }
        Some(secs) => Duration::from_secs(secs),
    };
    Ok(Def {
        command: file.command,
        timeout,
    })
}

/// The main checkout's check: `None` without the file.
async fn read(fs: &dyn FsService, root: &Path) -> Result<Option<Def>, String> {
    let path = RelPath::parse(FILE).map_err(|e| e.message)?;
    match fs.read_file(root.to_path_buf(), path).await {
        Ok(file) => match file.body {
            FileBody::Text { text } => parse(&text).map(Some),
            _ => Err(format!("{FILE} is not a text file")),
        },
        Err(e) if e.code == ErrorCode::NotFound => Ok(None),
        Err(e) => Err(format!("{FILE} could not be read: {}", e.message)),
    }
}

/// Runs the project's check for `entry`, a worker's item it just asked to
/// have reviewed, in the background. An earlier run of that worker is
/// killed. Without a check nothing runs and nothing shows.
pub(super) async fn start(core: &Core, entry: &Entry) {
    let session = entry.session.clone();
    let root = match core.root(&entry.project) {
        Ok(root) => root,
        Err(_) => return,
    };
    let def = read(core.fs_service(), &root).await;
    // An ended session's workspace waits for the person: nothing changes it.
    let idle = core
        .sessions()
        .find(&session, |s| {
            *s.status() == AgentStatus::Idle || s.status().is_ended()
        })
        .unwrap_or(false);
    let cleared = if !matches!(def, Ok(Some(_))) {
        let saved = match &def {
            Err(_) => Some(Saved {
                check: ReviewCheck {
                    state: ReviewCheckState::Failed,
                    command: FILE.to_owned(),
                    detail: Some("the check file is broken".to_owned()),
                    stale: false,
                },
                tree: None,
            }),
            _ => None,
        };
        save(core, &entry.id, saved)
    } else {
        false
    };
    let state = core.orchestration_state();
    let launch_now = {
        let mut guard = state.lock();
        guard.checks_made += 1;
        let generation = guard.checks_made;
        let run = |command: String, state, detail, log, pending| Run {
            review: entry.id.clone(),
            generation,
            command,
            state,
            detail,
            log,
            pending,
            task: None,
        };
        let (new, launch_now) = match def {
            Ok(None) => (None, None),
            Ok(Some(def)) => {
                let command = def.command.join(" ");
                let (pending, now) = if idle {
                    (None, Some((def, generation)))
                } else {
                    (Some((def, entry.clone())), None)
                };
                let running = ReviewCheckState::Running;
                (
                    Some(run(command, running, None, VecDeque::new(), pending)),
                    now,
                )
            }
            Err(why) => (
                Some(run(
                    FILE.to_owned(),
                    ReviewCheckState::Failed,
                    Some("the check file is broken".to_owned()),
                    VecDeque::from([why]),
                    None,
                )),
                None,
            ),
        };
        let shown = new.is_some();
        // The earlier run, if any, is killed as it drops.
        let had = match new {
            Some(new) => guard.checks.insert(session.clone(), new).is_some(),
            None => guard.checks.remove(&session).is_some(),
        };
        if !shown && !had && !cleared {
            return;
        }
        launch_now
    };
    changed(core, entry);
    if let Some((def, generation)) = launch_now {
        launch(core, entry, def, generation);
    }
}

/// Runs `def` for `entry` in the background as run `generation`.
fn launch(core: &Core, entry: &Entry, def: Def, generation: u64) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let step = SetupStep {
        line: CommandLine {
            program: PathBuf::from(&def.command[0]),
            args: def.command[1..].iter().map(Into::into).collect(),
            env: Vec::new(),
            cwd: entry.workspace.dir.clone(),
        },
        timeout: def.timeout,
    };
    let sink: Arc<dyn SetupSink> = Arc::new(Sink {
        core: core.downgrade(),
        session: entry.session.clone(),
        generation,
    });
    let (core2, entry2) = (core.clone(), entry.clone());
    let task = runtime.spawn(async move {
        // What it judges: the workspace as the turn left it.
        let tree = review::listed(&core2, &entry2, None)
            .await
            .ok()
            .and_then(|(_, _, tree)| tree);
        // A direct session works in the main checkout, which has its own pod.
        let session = (entry2.mode != SessionMode::Direct).then_some(&entry2.session);
        let end = crate::servers::run_steps(
            &core2,
            &entry2.project,
            session,
            entry2.workspace.dir.clone(),
            vec![step],
            sink,
        )
        .await;
        finish(&core2, &entry2, generation, end, def.timeout, tree);
    });
    let state = core.orchestration_state();
    let mut guard = state.lock();
    match guard.checks.get_mut(&entry.session) {
        Some(run) if run.generation == generation && run.unfinished() => {
            run.task = Some(task.abort_handle());
        }
        // Dropped or replaced while it was spawned.
        _ => task.abort(),
    }
}

/// Records how the run `generation` ended, unless a newer one replaced it.
fn finish(
    core: &Core,
    entry: &Entry,
    generation: u64,
    end: Result<SetupEnd, ProtocolError>,
    timeout: Duration,
    tree: Option<String>,
) {
    let (state, detail) = match end {
        Ok(SetupEnd::Done) => (ReviewCheckState::Passed, None),
        Ok(SetupEnd::Failed {
            code: Some(code), ..
        }) => (ReviewCheckState::Failed, Some(format!("exit code {code}"))),
        Ok(SetupEnd::Failed {
            reason: Some(reason),
            ..
        }) => (
            ReviewCheckState::Failed,
            Some(format!("could not start: {reason}")),
        ),
        Ok(SetupEnd::Failed { .. }) => (
            ReviewCheckState::Failed,
            Some("ended by a signal".to_owned()),
        ),
        Ok(SetupEnd::TimedOut { .. }) => (
            ReviewCheckState::Failed,
            Some(format!("timed out after {} s", timeout.as_secs())),
        ),
        Err(err) if err.code == ErrorCode::Unavailable => {
            (ReviewCheckState::NotRun, Some(err.message))
        }
        Err(err) => (ReviewCheckState::Failed, Some(err.message)),
    };
    {
        let orchestration = core.orchestration_state();
        let mut guard = orchestration.lock();
        let Some(run) = guard.checks.get_mut(&entry.session) else {
            return;
        };
        if run.generation != generation {
            return;
        }
        run.state = state;
        run.detail = detail.clone();
        // Finished: nothing left to abort.
        run.task = None;
        let saved = Saved {
            check: ReviewCheck {
                state,
                command: run.command.clone(),
                detail,
                stale: false,
            },
            tree,
        };
        drop(guard);
        save(core, &entry.id, Some(saved));
    }
    changed(core, entry);
}

/// Saves the item's finished check, or forgets it; whether that changed
/// anything. The caller tells the clients.
fn save(core: &Core, review: &ReviewId, saved: Option<Saved>) -> bool {
    let orchestration = core.orchestration_state();
    let mut state = orchestration.lock();
    let Some(entry) = state.reviews.iter_mut().find(|r| &r.id == review) else {
        return false;
    };
    if entry.check.is_none() && saved.is_none() {
        return false;
    }
    entry.check = saved;
    orchestration.save(&state);
    true
}

fn changed(core: &Core, entry: &Entry) {
    core.emit(ReviewEvent::Changed {
        project: entry.project.clone(),
        id: entry.id.clone(),
    });
}

/// What `entry`'s item shows of its latest check: a run in progress, else
/// the saved result, stale when the workspace's look `now` is not the one
/// it judged.
pub(super) fn view(core: &Core, entry: &Entry, now: Option<&String>) -> Option<Box<ReviewCheck>> {
    {
        let state = core.orchestration_state();
        let guard = state.lock();
        if let Some(run) = guard
            .checks
            .get(&entry.session)
            .filter(|r| r.review == entry.id && r.unfinished())
        {
            return Some(Box::new(ReviewCheck {
                state: run.state,
                command: run.command.clone(),
                detail: run.detail.clone(),
                stale: false,
            }));
        }
    }
    let saved = entry.check.as_ref()?;
    let stale = matches!((&saved.tree, now), (Some(then), Some(now)) if then != now);
    Some(Box::new(ReviewCheck {
        stale,
        ..saved.check.clone()
    }))
}

/// `review.check_log`: the last lines of the item's check.
pub(super) fn log(
    core: &Core,
    session: &SessionId,
    review: &ReviewId,
) -> Result<Vec<String>, ProtocolError> {
    let state = core.orchestration_state();
    let guard = state.lock();
    if let Some(run) = guard.checks.get(session).filter(|r| &r.review == review) {
        return Ok(run.log.iter().cloned().collect());
    }
    drop(guard);
    match review::find(core, review)?.check {
        Some(_) => Ok(vec![
            "Ovra keeps a check's output only until it restarts.".to_owned(),
        ]),
        None => Err(super::not_found(format!("a check of review {}", review.0))),
    }
}

/// Kills the worker's check if it has not finished (its review was
/// decided, or the worker ended); a finished result stays.
pub(super) fn forget(core: &Core, session: &SessionId) {
    let state = core.orchestration_state();
    let dropped = {
        let mut guard = state.lock();
        match guard.checks.get(session) {
            Some(run) if run.unfinished() => guard.checks.remove(session),
            _ => None,
        }
    };
    if let Some(run) = dropped {
        tell(core, session, &run);
    }
}

/// A waiting check starts when the agent's turn ends. Once the agent works
/// again after that, an unfinished check judges files that are changing,
/// and goes (a finished one stays, and shows as stale once the files
/// differ); once the agent ends, an unfinished check goes with it. A
/// person's own session's turn that left changes the check has not judged
/// starts one.
pub(super) fn on_status(core: &Core, session: &SessionId, status: &AgentStatus) {
    if status.is_ended() {
        forget(core, session);
        return;
    }
    if *status == AgentStatus::Idle && own(core, session) {
        let core = core.clone();
        let session = session.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { after_turn(&core, &session).await });
        }
    }
    let state = core.orchestration_state();
    let (launch_now, dropped) = {
        let mut guard = state.lock();
        let Some(run) = guard.checks.get_mut(session) else {
            return;
        };
        match status {
            AgentStatus::Idle => {
                let generation = run.generation;
                (run.pending.take().map(|p| (p, generation)), None)
            }
            AgentStatus::Working | AgentStatus::Starting
                if run.pending.is_none() && run.unfinished() =>
            {
                (None, guard.checks.remove(session))
            }
            _ => (None, None),
        }
    };
    if let Some(((def, entry), generation)) = launch_now {
        launch(core, &entry, def, generation);
    }
    if let Some(run) = dropped {
        tell(core, session, &run);
    }
}

/// A session the person started themselves: not a worker of a task with a
/// coordinator, which asks for review when its part is done.
fn own(core: &Core, session: &SessionId) -> bool {
    super::tasks::of_worker(core, session).is_none_or(|task| task.solo)
}

/// Runs the check for a person's own session whose turn left changes by the
/// agent that the check has not judged yet.
async fn after_turn(core: &Core, session: &SessionId) {
    let Some(entry) = review::of_session(core, session) else {
        return;
    };
    if entry.state != ReviewState::Open {
        return;
    }
    let busy = {
        let state = core.orchestration_state();
        let guard = state.lock();
        guard.checks.get(session).is_some_and(Run::unfinished)
    };
    if busy {
        return;
    }
    let Ok((files, _, tree)) = review::listed(core, &entry, None).await else {
        return;
    };
    if !files.iter().any(|f| f.by != ChangedBy::Outside) {
        return;
    }
    let judged = entry.check.as_ref().is_some_and(|saved| {
        saved.tree.is_some() && saved.tree == tree && saved.check.state != ReviewCheckState::NotRun
    });
    if !judged {
        start(core, &entry).await;
    }
}

fn tell(core: &Core, session: &SessionId, run: &Run) {
    if let Ok(project) = core.sessions().find(session, |s| s.info().project) {
        core.emit(ReviewEvent::Changed {
            project,
            id: run.review.clone(),
        });
    }
}

/// Collects the run's output while it is the worker's latest.
#[derive(Debug)]
struct Sink {
    core: WeakCore,
    session: SessionId,
    generation: u64,
}

impl SetupSink for Sink {
    fn step(&self, _: usize) {}

    fn output(&self, _: usize, lines: Vec<String>) {
        let Some(core) = self.core.upgrade() else {
            return;
        };
        let state = core.orchestration_state();
        let mut guard = state.lock();
        let Some(run) = guard.checks.get_mut(&self.session) else {
            return;
        };
        if run.generation != self.generation {
            return;
        }
        for line in lines {
            if run.log.len() == MAX_LINES {
                run.log.pop_front();
            }
            run.log.push_back(cut(line));
        }
    }
}

fn cut(mut line: String) -> String {
    if let Some((at, _)) = line.char_indices().nth(MAX_LINE) {
        line.truncate(at);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_gives_a_command_and_a_timeout() {
        let def = parse(r#"{"command": ["just", "check"], "timeout": 900}"#);
        assert_eq!(
            def,
            Ok(Def {
                command: vec!["just".into(), "check".into()],
                timeout: Duration::from_secs(900),
            })
        );
        let def = parse(r#"{"command": ["npm", "test"]}"#).map(|d| d.timeout);
        assert_eq!(def, Ok(DEFAULT_TIMEOUT));
    }

    #[test]
    fn a_broken_file_says_what_is_wrong() {
        for (text, why) in [
            (r#"{"command": []}"#, "the command is empty"),
            (r#"{"command": [""]}"#, "the program is empty"),
            (r#"{"command": ["a\u0000"]}"#, "NUL byte"),
            (r#"{"command": ["x"], "timeout": 0}"#, "at least 1 second"),
            (r#"{"command": ["x"], "timeout": 3601}"#, "at most 3600"),
            (r#"{"command": ["x"], "shell": true}"#, "unknown field"),
            ("not json", "expected"),
        ] {
            let err = parse(text).err().unwrap_or_default();
            assert!(err.starts_with(FILE) && err.contains(why), "{text}: {err}");
        }
    }

    #[test]
    fn long_lines_are_cut_on_a_character() {
        assert_eq!(cut("é".repeat(MAX_LINE + 5)).chars().count(), MAX_LINE);
        assert_eq!(cut("short".into()), "short");
    }
}
