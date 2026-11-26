//! Checkpoints of a direct session (`docs/specs/checkpoints.md`): a
//! snapshot before and after every turn, and going back to before one.
//!
//! Going back follows the same rule as reject: a file is set back only if
//! every change to it since the checkpoint was the agent's. The tracker
//! notes when each file was last changed outside the agent (its `epoch`,
//! which every turn's before snapshot counts up), so a file the person
//! touched before the checkpoint can still go back to how it was then,
//! with their change, and one they touched after it is kept.

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Direct, Tracker, Writer, between, conflict, delta_paths, lock};
use crate::path::RelPath;
use crate::protected;
use crate::protocol::agents::{Checkpoint, SessionId};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{Result, Worktrees};

/// Checkpoints kept per session; older ones drop.
pub(super) const MAX_CHECKPOINTS: usize = 100;

/// A turn's start or end, as reported.
#[derive(Debug)]
pub(super) enum Edge {
    Start {
        turn: u32,
        at_ms: u64,
        message: String,
    },
    End {
        turn: u32,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Turns {
    /// Oldest first.
    list: VecDeque<Turn>,
    /// The latest turn started, and whether it still runs.
    current: u32,
    /// Not kept: after a restart no turn runs.
    #[serde(skip)]
    running: bool,
    /// What the last go-back restored, until the next turn starts.
    undo: Option<GoneBack>,
    /// What the agent is told with its next message.
    note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Turn {
    turn: u32,
    at_ms: u64,
    message: String,
    /// The snapshot taken when its message was sent.
    before: String,
    /// The tracker's epoch from that snapshot on.
    epoch: u64,
    /// Files that differ between before and after it; 0 until it ends.
    files_changed: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoneBack {
    /// The tree just before the go-back.
    snapshot: String,
    /// The checkpoint it went back to.
    before: String,
    #[serde(with = "super::kept::rel_paths")]
    restored: Vec<RelPath>,
}

impl Turns {
    /// The review was decided: the checkpoints go with it, and their
    /// snapshots can be pruned.
    pub(super) fn decided(&mut self) {
        self.list.clear();
        self.undo = None;
    }
}

impl Writer {
    /// Takes the snapshot of a turn edge. Without a tracker (no baseline,
    /// or a rejected session) there is nothing to go back with.
    pub(super) async fn record(
        &self,
        worktrees: &dyn Worktrees,
        tracker: Option<&mut Tracker>,
        edge: Edge,
    ) {
        let Some(t) = tracker else {
            return;
        };
        let dir = self.dir.clone();
        match edge {
            Edge::Start {
                turn,
                at_ms,
                message,
            } => {
                let Ok(before) = worktrees.snapshot(dir.clone()).await else {
                    return;
                };
                // What changed while the agent was idle happened before
                // this checkpoint.
                if t.open.is_none() {
                    t.catch_up(worktrees, &dir, Some(&before)).await;
                    t.last = Some(before.clone());
                }
                t.epoch += 1;
                let mut turns = lock(&self.turns);
                turns.list.push_back(Turn {
                    turn,
                    at_ms,
                    message,
                    before,
                    epoch: t.epoch,
                    files_changed: 0,
                });
                while turns.list.len() > MAX_CHECKPOINTS {
                    turns.list.pop_front();
                }
            }
            Edge::End { turn } => {
                let before = lock(&self.turns)
                    .list
                    .back()
                    .filter(|t| t.turn == turn)
                    .map(|t| t.before.clone());
                let Some(before) = before else {
                    return;
                };
                let Ok(after) = worktrees.snapshot(dir.clone()).await else {
                    return;
                };
                let changed = worktrees.diff(dir, before, after).await;
                let count = changed.map_or(0, |d| u32::try_from(d.len()).unwrap_or(u32::MAX));
                if let Some(last) = lock(&self.turns).list.back_mut()
                    && last.turn == turn
                {
                    last.files_changed = count;
                }
            }
        }
    }
}

impl Direct {
    /// Message `turn` (from 1) was sent to the agent: its before snapshot is
    /// taken before any of its tools runs. Ends what a go-back left to undo.
    /// Answers whether the session works directly, and then the caller runs
    /// [`Self::catch_up`] soon.
    pub(crate) fn turn_started(&self, session: &SessionId, turn: u32, message: String) -> bool {
        let Some((_, writer)) = self.find(session) else {
            return false;
        };
        {
            let mut turns = lock(&writer.turns);
            turns.current = turn;
            turns.running = true;
            turns.undo = None;
            turns.note = None;
        }
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        lock(&writer.edges).push(Edge::Start {
            turn,
            at_ms,
            message,
        });
        true
    }

    /// The running turn ended: its result came, or the session stopped or
    /// failed. Answers whether there is an after snapshot to take, which
    /// [`Self::catch_up`] then does.
    pub(crate) fn turn_ended(&self, session: &SessionId) -> bool {
        let Some((_, writer)) = self.find(session) else {
            return false;
        };
        let turn = {
            let mut turns = lock(&writer.turns);
            if !std::mem::take(&mut turns.running) {
                return false;
            }
            turns.current
        };
        lock(&writer.edges).push(Edge::End { turn });
        true
    }

    /// What the agent's next message starts with, after a go-back.
    pub(crate) fn note(&self, session: &SessionId) -> Option<String> {
        let (_, writer) = self.find(session)?;
        lock(&writer.turns).note.clone()
    }

    /// Whether the last go-back of `session` can still be undone.
    pub(crate) fn can_undo_go_back(&self, session: &SessionId) -> bool {
        self.find(session)
            .is_some_and(|(_, w)| lock(&w.turns).undo.is_some())
    }

    /// The turns that can be gone back to, newest first. None for a session
    /// direct mode no longer tracks (reviewed, or it changed nothing).
    pub(crate) async fn checkpoints(&self, session: &SessionId) -> Result<Vec<Checkpoint>> {
        let Some((_, writer)) = self.find(session) else {
            return Ok(Vec::new());
        };
        let tracker = writer.lock(&*self.worktrees).await;
        if tracker.is_none() {
            return Err(untracked(&writer));
        }
        let turns = lock(&writer.turns);
        Ok(turns
            .list
            .iter()
            .rev()
            .map(|t| Checkpoint {
                turn: t.turn,
                at_ms: t.at_ms,
                message: t.message.clone(),
                files_changed: t.files_changed,
            })
            .collect())
    }

    /// Sets the agent's changes since before `turn` back, keeping every
    /// file changed outside the agent since. Answers the files restored and
    /// those kept.
    pub(crate) async fn go_back(
        &self,
        session: &SessionId,
        turn: u32,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.find(session).ok_or_else(untracked_session)?;
        let dir = writer.dir.clone();
        let mut tracker = writer.lock(worktrees).await;
        let t = tracker.as_mut().ok_or_else(|| untracked(&writer))?;
        idle(&writer, t)?;
        let (before, epoch, latest) = {
            let turns = lock(&writer.turns);
            let found = turns.list.iter().find(|c| c.turn == turn);
            let found = found.ok_or_else(|| dropped(turn, turns.list.front()))?;
            (found.before.clone(), found.epoch, turns.current)
        };
        let now = worktrees.snapshot(dir.clone()).await?;
        t.catch_up(worktrees, &dir, Some(&now)).await;
        let mut back = Vec::new();
        let mut kept = Vec::new();
        for delta in worktrees
            .diff(dir.clone(), before.clone(), now.clone())
            .await?
        {
            if protected::is_placeholder(&delta) {
                continue;
            }
            let paths: Vec<String> = delta_paths(&delta).collect();
            let mine = paths
                .iter()
                .all(|p| t.agent.contains_key(p) && t.touched.get(p).is_none_or(|e| *e < epoch));
            if mine {
                back.extend(paths);
            } else {
                kept.push(delta.path);
            }
        }
        let back = paths(&back)?;
        let left = t.put_back(worktrees, &dir, &now, &before, &back).await?;
        let (restored, failed): (Vec<RelPath>, Vec<RelPath>) =
            back.into_iter().partition(|p| !left.contains(p.as_str()));
        kept.extend(failed.iter().map(|p| p.as_str().to_owned()));
        kept.sort();
        kept.dedup();
        let names = names(&restored);
        {
            let mut turns = lock(&writer.turns);
            if !restored.is_empty() {
                turns.note = Some(note(turn, latest, &kept));
                turns.undo = Some(GoneBack {
                    snapshot: now,
                    before,
                    restored,
                });
            }
        }
        writer.keep(&tracker);
        Ok((names, kept))
    }

    /// Puts back what the last go-back restored, from the snapshot taken
    /// just before it. A file changed since the go-back is kept and listed.
    pub(crate) async fn undo_go_back(
        &self,
        session: &SessionId,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let worktrees = &*self.worktrees;
        let (_, writer) = self.find(session).ok_or_else(untracked_session)?;
        let dir = writer.dir.clone();
        let mut tracker = writer.lock(worktrees).await;
        let t = tracker.as_mut().ok_or_else(|| untracked(&writer))?;
        idle(&writer, t)?;
        let none = || conflict("there is no go-back to undo");
        let before = lock(&writer.turns).undo.as_ref().map(|u| u.before.clone());
        let before = before.ok_or_else(none)?;
        let now = worktrees.look(dir.clone()).await?;
        // Restored files are as in `before` until someone changes them.
        let changed = worktrees.diff(dir.clone(), before, now.clone()).await?;
        // Taken only now (under the tracker's lock, so it is still there):
        // an error above leaves it to try again.
        let undo = lock(&writer.turns).undo.take().ok_or_else(none)?;
        let changed: BTreeSet<String> = changed.iter().flat_map(delta_paths).collect();
        let (back, mut kept): (Vec<RelPath>, Vec<RelPath>) = undo
            .restored
            .into_iter()
            .partition(|p| !changed.contains(p.as_str()));
        t.catch_up(worktrees, &dir, Some(&now)).await;
        let left = t
            .put_back(worktrees, &dir, &now, &undo.snapshot, &back)
            .await?;
        let (restored, failed): (Vec<RelPath>, Vec<RelPath>) =
            back.into_iter().partition(|p| !left.contains(p.as_str()));
        kept.extend(failed);
        lock(&writer.turns).note = None;
        writer.keep(&tracker);
        Ok((names(&restored), names(&kept)))
    }
}

impl Tracker {
    /// Puts `paths` back as they are in `from`, on the agent's behalf: they
    /// stay the agent's, and whatever else changed since `now` (the tree
    /// just before, already caught up with) counts as outside. Answers the
    /// paths that still differ from `from` afterwards.
    async fn put_back(
        &mut self,
        worktrees: &dyn Worktrees,
        dir: &Path,
        now: &String,
        from: &str,
        paths: &[RelPath],
    ) -> Result<BTreeSet<String>> {
        if paths.is_empty() {
            return Ok(BTreeSet::new());
        }
        let d = dir.to_path_buf();
        worktrees
            .restore(d.clone(), from.to_owned(), paths.to_vec())
            .await?;
        let post = worktrees.look(d.clone()).await.ok();
        let ours: BTreeSet<&str> = paths.iter().map(RelPath::as_str).collect();
        match &post {
            Some(post) => match between(worktrees, d.clone(), now, post).await {
                Ok(changed) => {
                    for path in changed.iter().flat_map(delta_paths) {
                        if !ours.contains(path.as_str()) {
                            self.changed_outside(path);
                        }
                    }
                }
                Err(_) => self.taint_all(),
            },
            None => self.taint_all(),
        }
        self.last = post.clone();
        let left = match post {
            Some(post) => worktrees.diff(d, from.to_owned(), post).await?,
            None => worktrees.changes(d, from.to_owned()).await?,
        };
        Ok(left
            .iter()
            .flat_map(delta_paths)
            .filter(|p| ours.contains(p.as_str()))
            .collect())
    }
}

/// No turn and no tool of the agent runs.
fn idle(writer: &Writer, t: &Tracker) -> Result<()> {
    if lock(&writer.turns).running || t.running() {
        return Err(conflict(
            "the agent is working on a turn; stop it or wait for the turn to end first",
        ));
    }
    Ok(())
}

fn paths(names: &[String]) -> Result<Vec<RelPath>> {
    names.iter().map(|p| RelPath::parse(p)).collect()
}

fn names(paths: &[RelPath]) -> Vec<String> {
    paths.iter().map(|p| p.as_str().to_owned()).collect()
}

/// What the agent is told after a go-back to `from`, when `to` was the
/// latest turn.
fn note(from: u32, to: u32, kept: &[String]) -> String {
    let undone = if from == to {
        format!("changes from turn {from} are undone")
    } else {
        format!("changes from turns {from} to {to} are undone")
    };
    let mut note =
        format!("The files were set back to how they were before your turn {from}; {undone}.");
    if !kept.is_empty() {
        note.push_str(&format!(
            " Files the person changed are kept: {}.",
            kept.join(", ")
        ));
    }
    note
}

fn untracked_session() -> ProtocolError {
    conflict(
        "Ovra keeps no checkpoints for this session any more (its changes were already \
         reviewed, or it changed no files), so nothing was restored",
    )
}

/// A direct session without attribution: no baseline, or rejected.
fn untracked(writer: &Writer) -> ProtocolError {
    if lock(&writer.undo).is_some() {
        return conflict("the session's changes were rejected; undo the reject to go back");
    }
    ProtocolError::new(
        ErrorCode::Unavailable,
        "Ovra keeps no checkpoints for this session: it could not snapshot the project \
         (only git repositories have them)",
    )
}

fn dropped(turn: u32, oldest: Option<&Turn>) -> ProtocolError {
    let reason = match oldest {
        Some(oldest) => format!("the oldest kept is turn {}", oldest.turn),
        None => "none are kept".to_owned(),
    };
    ProtocolError::new(
        ErrorCode::NotFound,
        format!(
            "there is no checkpoint of turn {turn} ({reason}; the last {MAX_CHECKPOINTS} turns \
             since the last review are kept)"
        ),
    )
}
