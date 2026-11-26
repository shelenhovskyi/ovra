//! The project's check before review (`docs/specs/review-check.md`): when a
//! worker asks for review, the command in `.ovra/check.json` of the main
//! checkout runs once in the worker's worktree, in that worktree's pod
//! ([`crate::servers::run_steps`]), so it gets no more than the worker has.
//! `.ovra` is protected, so only the person defines what runs. Results stay
//! in memory with the review item and go once the worker works again.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::review::Entry;
use crate::Core;
use crate::core::WeakCore;
use crate::path::RelPath;
use crate::protocol::agents::{AgentStatus, SessionId};
use crate::protocol::fs::FileBody;
use crate::protocol::review::{ReviewCheck, ReviewCheckState, ReviewEvent, ReviewId};
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

/// One run, kept per worker: only its latest request counts.
#[derive(Debug)]
pub(super) struct Run {
    review: ReviewId,
    generation: u64,
    command: String,
    state: ReviewCheckState,
    detail: Option<String>,
    log: VecDeque<String>,
    /// The worker's turn ended since the run started: when it works again,
    /// its files are no longer what the check judged.
    idle_seen: bool,
    task: Option<tokio::task::AbortHandle>,
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
    let idle = core
        .sessions()
        .find(&session, |s| *s.status() == AgentStatus::Idle)
        .unwrap_or(false);
    let state = core.orchestration_state();
    let generation = {
        let mut guard = state.lock();
        guard.checks_made += 1;
        let generation = guard.checks_made;
        let run = |command: String, state, detail, log| Run {
            review: entry.id.clone(),
            generation,
            command,
            state,
            detail,
            log,
            idle_seen: idle,
            task: None,
        };
        let new = match &def {
            Ok(None) => None,
            Ok(Some(def)) => Some(run(
                def.command.join(" "),
                ReviewCheckState::Running,
                None,
                VecDeque::new(),
            )),
            Err(why) => Some(run(
                FILE.to_owned(),
                ReviewCheckState::Failed,
                Some("the check file is broken".to_owned()),
                VecDeque::from([why.clone()]),
            )),
        };
        let shown = new.is_some();
        let had = match new {
            Some(new) => guard.checks.insert(session.clone(), new).is_some(),
            None => guard.checks.remove(&session).is_some(),
        };
        if !shown && !had {
            return;
        }
        generation
    };
    changed(core, entry);
    let Ok(Some(def)) = def else { return };
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
        session: session.clone(),
        generation,
    });
    let (core2, entry2) = (core.clone(), entry.clone());
    let task = runtime.spawn(async move {
        let end = crate::servers::run_steps(
            &core2,
            &entry2.project,
            &entry2.session,
            entry2.workspace.dir.clone(),
            vec![step],
            sink,
        )
        .await;
        finish(&core2, &entry2, generation, end, def.timeout);
    });
    let mut guard = state.lock();
    match guard.checks.get_mut(&session) {
        Some(run) if run.generation == generation && run.state == ReviewCheckState::Running => {
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
        run.detail = detail;
        // Finished: nothing left to abort.
        run.task = None;
    }
    changed(core, entry);
}

fn changed(core: &Core, entry: &Entry) {
    core.emit(ReviewEvent::Changed {
        project: entry.project.clone(),
        id: entry.id.clone(),
    });
}

/// What the item `review` shows of its worker's latest check.
pub(super) fn view(
    core: &Core,
    session: &SessionId,
    review: &ReviewId,
) -> Option<Box<ReviewCheck>> {
    let state = core.orchestration_state();
    let guard = state.lock();
    let run = guard.checks.get(session).filter(|r| &r.review == review)?;
    Some(Box::new(ReviewCheck {
        state: run.state,
        command: run.command.clone(),
        detail: run.detail.clone(),
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
    guard
        .checks
        .get(session)
        .filter(|r| &r.review == review)
        .map(|r| r.log.iter().cloned().collect())
        .ok_or_else(|| super::not_found(format!("a check of review {}", review.0)))
}

/// A worker's turn ended, or it works again: once it works after its turn
/// ended, its check judged files that are gone, and goes.
pub(super) fn on_status(core: &Core, session: &SessionId, status: &AgentStatus) {
    let state = core.orchestration_state();
    let dropped = {
        let mut guard = state.lock();
        let Some(run) = guard.checks.get_mut(session) else {
            return;
        };
        match status {
            AgentStatus::Idle => {
                run.idle_seen = true;
                None
            }
            AgentStatus::Working | AgentStatus::Starting if run.idle_seen => {
                guard.checks.remove(session)
            }
            _ => None,
        }
    };
    let Some(run) = dropped else { return };
    let project = core.sessions().find(session, |s| s.info().project);
    if let Ok(project) = project {
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
