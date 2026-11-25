//! Direct mode (ADR 0005): one agent session works in the project's main
//! checkout. This module holds the "direct writer" lock, the session's
//! baseline snapshot, which files are the agent's, and approve and reject.
//!
//! Losing the person's work is the worst failure here, so every rule leans
//! one way: a file is reverted only with positive evidence that its current
//! content is exactly what the agent left and that nobody else changed it
//! before or after. Anything unclear is "changed outside the agent" or a
//! conflict, and is left alone.
//!
//! How the evidence is gathered. Looks at the tree split the session into
//! intervals. While any file-changing tool of the agent runs (`Write`,
//! `Edit`, `MultiEdit`, `NotebookEdit`, `Bash`), an interval is open; it
//! starts with a look at the tree (`pre`, a tree id written from a temporary
//! index) and ends with another (`post`). What changed
//! from `pre` to the end belongs to the agent if a `Bash` call ran in the
//! interval, or if a file tool named the file; the rest changed outside it.
//! Between intervals only the person (or a formatter, or git) changes files:
//! each new interval and each review first compares the tree with the last
//! `post` and marks what differs as changed outside. An agent file changed
//! outside, before or after the agent wrote it, is tainted: shown as changed
//! by both and never reverted.
//!
//! Tool events come from the `PreToolUse` hook (the CLI waits for it, so the
//! `pre` look is taken before the tool runs), from permission requests
//! (a tool waiting for the person is not running) and from tool results.
//! Late or missing events only make files look changed outside, which keeps
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use crate::path::RelPath;
use crate::protected;
use crate::protocol::agents::SessionId;
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{ChangedBy, FileChange};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{Delta, Result, Worktrees};

/// Direct sessions with changes still to review, by session. At most one
/// of a project's is active (its agent runs): the "direct writer" lock.
/// Ended ones stay until they are approved or rejected; to an earlier
/// session, a later one's changes are simply changes outside it.
#[derive(Debug)]
pub(crate) struct Direct {
    worktrees: Arc<dyn Worktrees>,
    writers: Mutex<BTreeMap<SessionId, Arc<Writer>>>,
}

#[derive(Debug)]
struct Writer {
    project: ProjectId,
    dir: PathBuf,
    /// The session's agent runs; cleared when it ends.
    active: Mutex<bool>,
    /// `None` while the baseline is being recorded, or if that failed.
    tracker: Arc<tokio::sync::Mutex<Option<Tracker>>>,
    /// Tool calls whose results arrived, not yet applied to the tracker.
    /// Results are reported synchronously, so they are queued here in order
    /// and applied before anything else touches the tracker.
    ended: Mutex<Vec<String>>,
    /// Files the person saved through Ovra since the tracker last looked:
    /// changed outside the agent, even while one of its tools runs.
    saved: Mutex<Vec<String>>,
    /// What the last reject reverted, so the person can undo it.
    undo: Mutex<Option<Undo>>,
}

/// Enough to put back what a reject reverted.
#[derive(Debug)]
struct Undo {
    /// The tree just before the revert.
    snapshot: String,
    /// The files the revert restored from `baseline`.
    reverted: Vec<RelPath>,
    baseline: String,
    /// The attribution before the reject, for an ended session.
    tracker: Tracker,
}

impl Writer {
    /// The tracker, with finished tools and the session's end applied.
    async fn lock(&self, worktrees: &dyn Worktrees) -> TrackerGuard<'_> {
        let mut tracker = self.tracker.lock().await;
        let saved = std::mem::take(&mut *lock(&self.saved));
        let ended = std::mem::take(&mut *lock(&self.ended));
        if let Some(t) = tracker.as_mut() {
            // Saves first: they happened before the results queued after them.
            for path in saved {
                t.saved(path);
            }
            let mut any = false;
            for id in ended {
                any |= t.tools.remove(&id).is_some();
            }
            if any {
                t.settle(worktrees, &self.dir).await;
            }
            if !*lock(&self.active) {
                t.tools.clear();
                t.open = None;
            }
        }
        tracker
    }

    /// Whether an ended session can be forgotten: nothing of the agent's to
    /// review, or no baseline to review it against.
    fn forgettable(&self) -> bool {
        if *lock(&self.active) || lock(&self.undo).is_some() {
            return false;
        }
        match self.tracker.try_lock() {
            Ok(tracker) => tracker.as_ref().is_none_or(|t| t.agent.is_empty()),
            Err(_) => false,
        }
    }
}

type TrackerGuard<'a> = tokio::sync::MutexGuard<'a, Option<Tracker>>;

enum Claim {
    /// The session was here before.
    Known(Arc<Writer>),
    /// A new writer, its tracker locked until the baseline is recorded.
    New(tokio::sync::OwnedMutexGuard<Option<Tracker>>),
}

/// What approve and reject did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Files left for the person: changed by the agent and by someone else.
    pub(crate) conflicts: Vec<String>,
    /// Approve: the commit made, if one was asked for and there was
    /// something to commit. Reject: a snapshot of the tree just before the
    /// revert, so nothing reverted is lost.
    pub(crate) snapshot: Option<String>,
    /// A snapshot of the tree after the decision, if one could be taken.
    pub(crate) base: Option<String>,
    /// Reject: the unified diff of what it reverted, for the agent to see
    /// when a comment continues the session.
    pub(crate) diff: String,
    /// Reject: the files it restored; undo: the files it put back.
    pub(crate) restored: Vec<String>,
    /// Approve and reject: the files as the decision saw them, read from
    /// `seen`, so the review keeps what was decided without reading again.
    pub(crate) files: Vec<FileChange>,
    pub(crate) seen: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn conflict(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::Conflict, message)
}

impl Direct {
    pub(crate) fn new(worktrees: Arc<dyn Worktrees>) -> Self {
        Self {
            worktrees,
            writers: Mutex::new(BTreeMap::new()),
        }
    }

    /// Takes the direct-writer lock of `project` for `session` and records
    /// its baseline of `dir`. A session that was here before (resumed, or
    /// continued by a review comment) keeps its baseline. Answers the
    /// baseline commit. When recording fails the session still holds the
    /// lock, untracked, until it ends.
    pub(crate) async fn begin(
        &self,
        project: &ProjectId,
        session: &SessionId,
        dir: PathBuf,
    ) -> Result<String> {
        let worktrees = &*self.worktrees;
        let mut guard = match self.claim(project, session, &dir)? {
            Claim::Known(writer) => {
                let mut tracker = writer.lock(worktrees).await;
                if let Some(t) = tracker.as_ref() {
                    return Ok(t.baseline.clone());
                }
                // Continued after a reject: a new start, and no more undo.
                *lock(&writer.undo) = None;
                let baseline = worktrees.snapshot(writer.dir.clone()).await?;
                *tracker = Some(Tracker::new(baseline.clone()));
                return Ok(baseline);
            }
            Claim::New(guard) => guard,
        };
        let baseline = worktrees.snapshot(dir).await?;
        *guard = Some(Tracker::new(baseline.clone()));
        Ok(baseline)
    }

    /// The synchronous half of [`Self::begin`]: takes the lock.
    fn claim(&self, project: &ProjectId, session: &SessionId, dir: &Path) -> Result<Claim> {
        let mut writers = lock(&self.writers);
        let other = writers
            .iter()
            .find(|(id, w)| *id != session && &w.project == project && *lock(&w.active));
        if let Some((id, _)) = other {
            return Err(conflict(format!(
                "another agent ({}) already works in this project's main checkout",
                id.0
            )));
        }
        writers.retain(|id, w| id == session || !w.forgettable());
        if let Some(writer) = writers.get(session).cloned() {
            *lock(&writer.active) = true;
            return Ok(Claim::Known(writer));
        }
        let tracker = Arc::new(tokio::sync::Mutex::new(None));
        let guard = tracker
            .clone()
            .try_lock_owned()
            .map_err(|_| conflict("the baseline is being recorded"))?;
        let writer = Arc::new(Writer {
            project: project.clone(),
            dir: dir.to_path_buf(),
            active: Mutex::new(true),
            tracker,
            ended: Mutex::new(Vec::new()),
            saved: Mutex::new(Vec::new()),
            undo: Mutex::new(None),
        });
        writers.insert(session.clone(), writer);
        Ok(Claim::New(guard))
    }

    /// The session holding the direct-writer lock of `project`, if any.
    pub(crate) fn writer(&self, project: &ProjectId) -> Option<SessionId> {
        lock(&self.writers)
            .iter()
            .find(|(_, w)| &w.project == project && *lock(&w.active))
            .map(|(id, _)| id.clone())
    }

    fn find(&self, session: &SessionId) -> Option<(ProjectId, Arc<Writer>)> {
        lock(&self.writers)
            .get(session)
            .map(|w| (w.project.clone(), w.clone()))
    }

    fn release(&self, session: &SessionId) {
        lock(&self.writers).remove(session);
    }

    /// After approve or reject: a session whose agent still runs starts
    /// over from the tree as it is now; an ended one is done. Answers that
    /// snapshot, the base for what the review shows from now on.
    async fn reviewed(
        &self,
        session: &SessionId,
        writer: &Writer,
        tracker: &mut Option<Tracker>,
    ) -> Option<String> {
        let base = self.worktrees.snapshot(writer.dir.clone()).await.ok();
        if *lock(&writer.active) {
            *tracker = base.clone().map(Tracker::new);
        } else {
            self.release(session);
        }
        base
    }

    /// The `PreToolUse` hook: the agent is about to run a tool. `input` is
    /// the tool's input. Waits for the `pre` look when it opens an
    /// interval; errors are swallowed because the tool must still run (its
    /// changes then look changed outside).
    pub(crate) async fn tool_started(
        &self,
        session: &SessionId,
        id: &str,
        name: &str,
        input: &Value,
    ) {
        let worktrees = &*self.worktrees;
        let Some((_, writer)) = self.find(session) else {
            return;
        };
        let Some(kind) = Kind::of(&writer.dir, name, input) else {
            return;
        };
        let mut tracker = writer.lock(worktrees).await;
        if let Some(t) = tracker.as_mut() {
            let tool = Tool {
                name: name.to_owned(),
                input: input.clone(),
                kind,
                waiting: false,
            };
            t.tools.insert(id.to_owned(), tool.clone());
            t.run(worktrees, &writer.dir, &tool).await;
        }
    }

    /// The tool asks the person: it is not running until they answer.
    /// Permission requests do not carry the tool call's id, so the tool is
    /// found by name and input.
    pub(crate) async fn tool_waiting(&self, session: &SessionId, name: &str, input: &Value) {
        let worktrees = &*self.worktrees;
        let Some((_, writer)) = self.find(session) else {
            return;
        };
        let mut tracker = writer.lock(worktrees).await;
        let Some(t) = tracker.as_mut() else {
            return;
        };
        if let Some(id) = t.find(name, input, false)
            && let Some(tool) = t.tools.get_mut(&id)
        {
            tool.waiting = true;
            t.settle(worktrees, &writer.dir).await;
        }
    }

    /// The person answered a permission request: an allowed tool runs now,
    /// a denied one never will.
    pub(crate) async fn tool_answered(
        &self,
        session: &SessionId,
        name: &str,
        input: &Value,
        allowed: bool,
    ) {
        let worktrees = &*self.worktrees;
        let Some((_, writer)) = self.find(session) else {
            return;
        };
        let mut tracker = writer.lock(worktrees).await;
        let Some(t) = tracker.as_mut() else {
            return;
        };
        let Some(id) = t.find(name, input, true) else {
            return;
        };
        if allowed {
            let Some(tool) = t.tools.get_mut(&id) else {
                return;
            };
            tool.waiting = false;
            let tool = tool.clone();
            t.run(worktrees, &writer.dir, &tool).await;
        } else {
            t.tools.remove(&id);
            t.settle(worktrees, &writer.dir).await;
        }
    }

    /// The tool's result arrived: it finished. Synchronous, so that a
    /// later event cannot overtake it; answers whether the session works
    /// directly, and then the caller runs [`Self::catch_up`] soon.
    pub(crate) fn tool_ended(&self, session: &SessionId, id: &str) -> bool {
        let Some((_, writer)) = self.find(session) else {
            return false;
        };
        lock(&writer.ended).push(id.to_owned());
        true
    }

    /// Applies finished tools reported by [`Self::tool_ended`].
    pub(crate) async fn catch_up(&self, session: &SessionId) {
        let worktrees = &*self.worktrees;
        if let Some((_, writer)) = self.find(session) {
            drop(writer.lock(worktrees).await);
        }
    }

    /// The session ended, which frees the direct-writer lock. Tools still
    /// running are dropped with their interval, so what they changed counts
    /// as changed outside. The session stays for review if it changed files.
    pub(crate) fn session_ended(&self, session: &SessionId) {
        if let Some((_, writer)) = self.find(session) {
            *lock(&writer.active) = false;
        }
    }

    /// The session's changes since its baseline, each marked with who made
    /// it.
    pub(crate) async fn files(&self, session: &SessionId) -> Result<Vec<FileChange>> {
        Ok(self.files_at(session).await?.0)
    }

    /// [`Self::files`], and the look of the folder they were read from, if
    /// it could be taken.
    pub(crate) async fn files_at(
        &self,
        session: &SessionId,
    ) -> Result<(Vec<FileChange>, Option<String>)> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.held(session)?;
        let mut tracker = writer.lock(worktrees).await;
        let t = tracker.as_mut().ok_or_else(not_direct)?;
        let now = worktrees.look(writer.dir.clone()).await.ok();
        let files = t.files(worktrees, &writer.dir, now.as_ref()).await?;
        Ok((files, now))
    }

    /// Keeps the changes. With `commit`, commits the
    /// agent's own files with that message; files changed by both are left
    /// uncommitted and listed as conflicts. Refused when a protected file
    /// changed, unless `guard` includes it as the person saw it: checked
    /// on the same look, under the same lock, as what is kept.
    pub(super) async fn approve(
        &self,
        session: &SessionId,
        commit: Option<String>,
        guard: &super::review::Guard,
    ) -> Result<Outcome> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.held(session)?;
        let mut tracker = writer.lock(worktrees).await;
        let t = tracker.as_mut().ok_or_else(not_direct)?;
        t.idle()?;
        let now = worktrees.look(writer.dir.clone()).await.ok();
        let mut files = t.files(worktrees, &writer.dir, now.as_ref()).await?;
        guard
            .admit(worktrees, &writer.dir, &mut files, now.as_ref())
            .await?;
        let mut outcome = Outcome {
            conflicts: Vec::new(),
            snapshot: None,
            base: None,
            diff: String::new(),
            restored: Vec::new(),
            files: Vec::new(),
            seen: now,
        };
        if let Some(message) = commit {
            let mine = paths(&files, ChangedBy::Agent)?;
            outcome.conflicts = names(&files, ChangedBy::Both);
            if !mine.is_empty() {
                let id = worktrees.commit(writer.dir.clone(), mine, message).await?;
                outcome.snapshot = Some(id);
            }
        }
        outcome.base = self.reviewed(session, &writer, &mut tracker).await;
        outcome.files = files;
        Ok(outcome)
    }

    /// Puts the agent's own files back as they were in the baseline. Files changed by both are left alone and listed
    /// as conflicts, and so is any file the revert could not restore.
    /// Refused while one of the agent's tools runs.
    pub(crate) async fn reject(&self, session: &SessionId) -> Result<Outcome> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.held(session)?;
        let mut tracker = writer.lock(worktrees).await;
        let t = tracker.as_mut().ok_or_else(not_direct)?;
        t.idle()?;
        let snapshot = worktrees.snapshot(writer.dir.clone()).await?;
        let files = t.files(worktrees, &writer.dir, Some(&snapshot)).await?;
        let mine = paths(&files, ChangedBy::Agent)?;
        let mut conflicts = names(&files, ChangedBy::Both);
        // The reverted files' parts of one diff; their order does not matter.
        let diff: String = worktrees
            .patches(
                writer.dir.clone(),
                t.baseline.clone(),
                snapshot.clone(),
                mine.clone(),
            )
            .await?
            .into_values()
            .collect();
        // Nothing to revert: the tree stays as in `snapshot`.
        let mut base = Some(snapshot.clone());
        if !mine.is_empty() {
            worktrees
                .restore(writer.dir.clone(), t.baseline.clone(), mine.clone())
                .await?;
            base = worktrees.snapshot(writer.dir.clone()).await.ok();
            let (dir, baseline) = (writer.dir.clone(), t.baseline.clone());
            let left = match &base {
                Some(base) => worktrees.diff(dir, baseline, base.clone()).await?,
                None => worktrees.changes(dir, baseline).await?,
            };
            let left: BTreeSet<String> = left.iter().flat_map(delta_paths).collect();
            conflicts.extend(
                mine.iter()
                    .map(|p| p.as_str().to_owned())
                    .filter(|p| left.contains(p)),
            );
        }
        conflicts.sort();
        conflicts.dedup();
        let reverted: Vec<RelPath> = mine
            .into_iter()
            .filter(|p| !conflicts.iter().any(|c| c == p.as_str()))
            .collect();
        let restored = reverted.iter().map(|p| p.as_str().to_owned()).collect();
        let old = tracker.take().ok_or_else(not_direct)?;
        *lock(&writer.undo) = Some(Undo {
            snapshot: snapshot.clone(),
            reverted,
            baseline: old.baseline.clone(),
            tracker: old,
        });
        // A running agent starts over; an ended session waits, untracked,
        // for an undo or a comment that continues it.
        if *lock(&writer.active) {
            *tracker = base.clone().map(Tracker::new);
        }
        Ok(Outcome {
            conflicts,
            snapshot: Some(snapshot.clone()),
            base,
            diff,
            restored,
            files,
            seen: Some(snapshot),
        })
    }

    /// Whether the last reject of `session` can still be undone.
    pub(crate) fn can_undo(&self, session: &SessionId) -> bool {
        self.find(session)
            .is_some_and(|(_, w)| lock(&w.undo).is_some())
    }

    /// Puts back what the last reject reverted, from the snapshot taken just
    /// before it. A file changed since the reject is left alone and listed
    /// as a conflict. An ended session gets its attribution back, and
    /// `base` is its baseline again.
    pub(crate) async fn undo_reject(&self, session: &SessionId) -> Result<Outcome> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.held(session)?;
        let mut tracker = writer.lock(worktrees).await;
        let undo = lock(&writer.undo)
            .take()
            .ok_or_else(|| conflict("there is no reject to undo"))?;
        let changed = worktrees
            .changes(writer.dir.clone(), undo.baseline.clone())
            .await;
        let back: Vec<RelPath> = match changed {
            Ok(changed) => {
                let changed: BTreeSet<String> = changed.iter().flat_map(delta_paths).collect();
                undo.reverted
                    .iter()
                    .filter(|p| !changed.contains(p.as_str()))
                    .cloned()
                    .collect()
            }
            Err(e) => {
                *lock(&writer.undo) = Some(undo);
                return Err(e);
            }
        };
        if !back.is_empty()
            && let Err(e) = worktrees
                .restore(writer.dir.clone(), undo.snapshot.clone(), back.clone())
                .await
        {
            *lock(&writer.undo) = Some(undo);
            return Err(e);
        }
        let conflicts = undo
            .reverted
            .iter()
            .filter(|p| !back.contains(p))
            .map(|p| p.as_str().to_owned())
            .collect();
        let base = if tracker.is_none() {
            let base = undo.baseline.clone();
            *tracker = Some(undo.tracker);
            Some(base)
        } else {
            None
        };
        Ok(Outcome {
            conflicts,
            snapshot: None,
            base,
            diff: String::new(),
            restored: back.iter().map(|p| p.as_str().to_owned()).collect(),
            files: Vec::new(),
            seen: None,
        })
    }

    /// The person saved `path` of `project` through Ovra (the editor): it is
    /// changed outside the agent even if one of the agent's tools runs.
    pub(crate) fn person_saved(&self, project: &ProjectId, path: &str) {
        for writer in lock(&self.writers).values() {
            if &writer.project == project {
                lock(&writer.saved).push(path.to_owned());
            }
        }
    }

    fn held(&self, session: &SessionId) -> Result<(ProjectId, Arc<Writer>)> {
        self.find(session).ok_or_else(not_direct)
    }
}

/// Gives a direct session its review item, comparing `dir` with `base`.
/// Called before the agent runs, so its hooks and config are as it found
/// them.
pub(crate) async fn review_from(
    core: &crate::Core,
    session: &SessionId,
    dir: PathBuf,
    base: String,
) {
    let git = core
        .worktrees()
        .git_files(dir.clone())
        .await
        .ok()
        .map(|g| g.files);
    super::review::set_workspace(core, session, super::Workspace { dir, base, git });
}

fn not_direct() -> ProtocolError {
    conflict(
        "Ovra does not know which of these files the agent changed (it was already \
         reviewed, or Ovra restarted since), so nothing was restored",
    )
}

/// A file-changing tool and what it may change.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// Any file: everything that changes while it runs is the agent's.
    Shell,
    /// The one file it names, relative to the project root. `None` when the
    /// path is outside the project: then nothing is the agent's.
    File(Option<String>),
}

/// Tools that change one named file, and the input field naming it.
const FILE_TOOLS: [(&str, &str); 4] = [
    ("Write", "file_path"),
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("NotebookEdit", "notebook_path"),
];

impl Kind {
    fn of(dir: &Path, name: &str, input: &Value) -> Option<Self> {
        if name == "Bash" {
            return Some(Self::Shell);
        }
        let (_, field) = FILE_TOOLS.iter().find(|(tool, _)| *tool == name)?;
        let path = input.get(field).and_then(Value::as_str);
        Some(Self::File(path.and_then(|p| relative(dir, p))))
    }
}

/// `path` (absolute, or relative to `dir`) relative to `dir`, resolved
/// lexically; `None` if it leaves `dir`.
fn relative(dir: &Path, path: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for component in dir.join(path).strip_prefix(dir).ok()?.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let joined = parts.join("/");
    RelPath::parse(&joined)
        .ok()
        .filter(|p| !p.is_root())
        .map(|p| p.as_str().to_owned())
}

#[derive(Debug, Clone)]
struct Tool {
    name: String,
    input: Value,
    kind: Kind,
    /// Waiting for the person to answer its permission request.
    waiting: bool,
}

/// An interval in which agent tools run.
#[derive(Debug)]
struct Interval {
    /// The tree it started from; `None` if the look failed, which makes
    /// nothing in the interval the agent's.
    pre: Option<String>,
    shell: bool,
    files: BTreeSet<String>,
    /// Files the person saved through Ovra meanwhile.
    saved: BTreeSet<String>,
}

#[derive(Debug)]
struct Tracker {
    baseline: String,
    /// The tree at the end of the last interval (the baseline at first): a
    /// look, not a snapshot, so it costs one git call. Git may prune it after
    /// two weeks; then the comparison fails and agent files count as tainted.
    /// `None` if taking it failed: then the next comparison is impossible
    /// and every agent file counts as tainted.
    last: Option<String>,
    /// Tools started and not finished, by tool call id.
    tools: BTreeMap<String, Tool>,
    open: Option<Interval>,
    /// The agent's files, and whether someone else changed them too.
    agent: BTreeMap<String, bool>,
    /// Files changed outside the agent since the baseline.
    outside: BTreeSet<String>,
}

/// What differs from `from` to `to`; nothing, without asking git, when both
/// name the same tree. That is the usual case between intervals.
async fn between(
    worktrees: &dyn Worktrees,
    dir: PathBuf,
    from: &String,
    to: &String,
) -> Result<Vec<Delta>> {
    if from == to {
        return Ok(Vec::new());
    }
    worktrees.diff(dir, from.clone(), to.clone()).await
}

fn delta_paths(delta: &Delta) -> impl Iterator<Item = String> + '_ {
    std::iter::once(delta.path.clone()).chain(delta.from.clone())
}

impl Tracker {
    fn new(baseline: String) -> Self {
        Self {
            last: Some(baseline.clone()),
            baseline,
            tools: BTreeMap::new(),
            open: None,
            agent: BTreeMap::new(),
            outside: BTreeSet::new(),
        }
    }

    fn find(&self, name: &str, input: &Value, waiting: bool) -> Option<String> {
        self.tools
            .iter()
            .find(|(_, t)| t.waiting == waiting && t.name == name && &t.input == input)
            .map(|(id, _)| id.clone())
    }

    fn running(&self) -> bool {
        self.tools.values().any(|t| !t.waiting)
    }

    fn idle(&self) -> Result<()> {
        if self.running() {
            return Err(conflict(
                "the agent is still running a tool; stop it or wait for it first",
            ));
        }
        Ok(())
    }

    /// Marks what changed since the last interval as changed outside:
    /// up to `now`, a tree of the working tree just taken, or else up
    /// to the working tree itself.
    async fn catch_up(&mut self, worktrees: &dyn Worktrees, dir: &Path, now: Option<&String>) {
        let dir = dir.to_path_buf();
        let changed = match (&self.last, now) {
            (Some(last), Some(now)) => between(worktrees, dir, last, now).await,
            (Some(last), None) => worktrees.changes(dir, last.clone()).await,
            (None, _) => Err(conflict("no snapshot to compare with")),
        };
        match changed {
            Ok(changed) => {
                for path in changed.iter().flat_map(delta_paths) {
                    self.changed_outside(path);
                }
            }
            // Unknown: whatever the agent wrote may have changed since.
            Err(_) => self.agent.values_mut().for_each(|t| *t = true),
        }
    }

    /// The person saved `path` through Ovra.
    fn saved(&mut self, path: String) {
        match self.open.as_mut() {
            Some(open) => {
                open.saved.insert(path);
            }
            None => self.changed_outside(path),
        }
    }

    fn changed_outside(&mut self, path: String) {
        match self.agent.get_mut(&path) {
            Some(tainted) => *tainted = true,
            None => {
                self.outside.insert(path);
            }
        }
    }

    /// `tool` runs now: opens an interval if none is open, and adds it.
    async fn run(&mut self, worktrees: &dyn Worktrees, dir: &Path, tool: &Tool) {
        if self.open.is_none() {
            let pre = worktrees.look(dir.to_path_buf()).await.ok();
            self.catch_up(worktrees, dir, pre.as_ref()).await;
            self.open = Some(Interval {
                pre,
                shell: false,
                files: BTreeSet::new(),
                saved: BTreeSet::new(),
            });
        }
        if let Some(open) = self.open.as_mut() {
            match &tool.kind {
                Kind::Shell => open.shell = true,
                Kind::File(Some(path)) => {
                    open.files.insert(path.clone());
                }
                Kind::File(None) => {}
            }
        }
    }

    /// Closes the open interval once no tool runs any more.
    async fn settle(&mut self, worktrees: &dyn Worktrees, dir: &Path) {
        if self.running() {
            return;
        }
        let Some(open) = self.open.take() else {
            return;
        };
        // `post` first: a change slipping in after it shows up again in the
        // next comparison, as changed outside.
        let post = worktrees.look(dir.to_path_buf()).await.ok();
        let dir = dir.to_path_buf();
        let changed = match (&open.pre, &post) {
            (Some(pre), Some(post)) => between(worktrees, dir, pre, post).await.ok(),
            (Some(pre), None) => worktrees.changes(dir, pre.clone()).await.ok(),
            (None, _) => None,
        };
        let Some(changed) = changed else {
            // Nothing is known about the interval: it was all outside.
            self.agent.values_mut().for_each(|t| *t = true);
            self.last = post;
            return;
        };
        for delta in &changed {
            let paths: Vec<String> = delta_paths(delta).collect();
            let mine = open.shell || paths.iter().any(|p| open.files.contains(p));
            // Saved by the person meanwhile: maybe the agent's too, but
            // never the agent's alone.
            let saved = paths.iter().any(|p| open.saved.contains(p));
            for path in paths {
                if mine && saved {
                    self.agent.insert(path, true);
                } else if mine {
                    let before = self.outside.contains(&path);
                    self.agent.entry(path).or_insert(before);
                } else {
                    self.changed_outside(path);
                }
            }
        }
        self.last = post;
    }

    /// The files changed since the baseline: up to `now`, a snapshot or a
    /// look just taken, or else up to the working tree.
    async fn files(
        &mut self,
        worktrees: &dyn Worktrees,
        dir: &Path,
        now: Option<&String>,
    ) -> Result<Vec<FileChange>> {
        if self.open.is_none() {
            self.catch_up(worktrees, dir, now).await;
        }
        let (dir, baseline) = (dir.to_path_buf(), self.baseline.clone());
        let changed = match now {
            Some(now) => worktrees.diff(dir, baseline, now.clone()).await?,
            None => worktrees.changes(dir, baseline).await?,
        };
        Ok(changed
            .into_iter()
            .map(|delta| {
                // Per path: `Some(tainted)` for the agent's, `None` for others.
                let marks: Vec<Option<bool>> = delta_paths(&delta)
                    .map(|p| self.agent.get(&p).copied())
                    .collect();
                let by = if marks.iter().all(Option::is_none) {
                    ChangedBy::Outside
                } else if marks.iter().all(|m| *m == Some(false)) {
                    ChangedBy::Agent
                } else {
                    // Tainted, or a rename pairing the agent's file with
                    // someone else's.
                    ChangedBy::Both
                };
                FileChange {
                    protected: protected::changes(&delta.path, delta.from.as_deref()),
                    path: delta.path,
                    kind: delta.kind,
                    from: delta.from,
                    by,
                    additions: delta.additions,
                    deletions: delta.deletions,
                    blocked: false,
                }
            })
            .collect())
    }
}

/// Refuses to keep `files` blindly when one of them is protected: they
/// are already on disk, so only the person's explicit choice keeps them.
pub(crate) fn protected_refused(files: &[FileChange]) -> Result<()> {
    let guarded: Vec<&str> = files
        .iter()
        .filter(|f| f.protected)
        .map(|f| f.path.as_str())
        .collect();
    if guarded.is_empty() {
        return Ok(());
    }
    Err(ProtocolError::new(
        ErrorCode::BadRequest,
        format!(
            "these changes include protected files ({}); look at their diffs and approve with include_protected",
            guarded.join(", ")
        ),
    ))
}

/// Every path (renames: both) of the files changed by `by`.
fn paths(files: &[FileChange], by: ChangedBy) -> Result<Vec<RelPath>> {
    files
        .iter()
        .filter(|f| f.by == by)
        .flat_map(|f| std::iter::once(&f.path).chain(&f.from))
        .map(|p| RelPath::parse(p))
        .collect()
}

fn names(files: &[FileChange], by: ChangedBy) -> Vec<String> {
    files
        .iter()
        .filter(|f| f.by == by)
        .map(|f| f.path.clone())
        .collect()
}

#[cfg(test)]
mod tests;
