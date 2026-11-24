//! Review items: one per session with a workspace. `open` until a person
//! approves or rejects it; a comment reopens it and continues the session.

use std::collections::{BTreeMap, BTreeSet};

use super::{Result, Workspace, conflict, not_found};
use crate::Core;
use crate::agents;
use crate::path::RelPath;
use crate::protocol::agents::{AgentStatus, AgentsRequest, SessionId, SessionMode};
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{
    ChangedBy, FileChange, ReviewEvent, ReviewId, ReviewItem, ReviewRequest, ReviewResponse,
    ReviewState,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};

/// Room left in a message for the words around a rejected diff.
const WRAPPER_BYTES: usize = 256;

/// A decided item keeps the diffs of at most this many files, and of at
/// most this many bytes in all; a bigger change keeps only its file list.
const KEPT_DIFF_FILES: usize = 50;
const KEPT_DIFF_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub(super) id: ReviewId,
    pub(super) project: ProjectId,
    pub(super) session: SessionId,
    mode: SessionMode,
    workspace: Workspace,
    pub(super) state: ReviewState,
    summary: Option<String>,
    /// The agent asked for a review, so the item shows even without changes.
    pub(super) requested: bool,
    /// The diff of the last reject, sent with the next comment.
    rejected: Option<String>,
    /// The files as they were when the person decided, kept as the record
    /// of what was approved or rejected.
    #[serde(default)]
    decided: Vec<FileChange>,
    /// The diffs of `decided`'s files, by path, if they were small enough.
    #[serde(default)]
    decided_diffs: BTreeMap<String, String>,
}

/// Records what `session`'s review compares, creating its item. Direct
/// sessions call it with their baseline (task 3.F), workers when they start.
pub(crate) fn set_workspace(core: &Core, session: &SessionId, workspace: Workspace) {
    let Ok((project, mode)) = core.sessions().find(session, |s| {
        let info = s.info();
        (info.project, info.mode)
    }) else {
        return;
    };
    let orchestration = core.orchestration_state();
    let mut state = orchestration.lock();
    if let Some(entry) = state
        .reviews
        .iter_mut()
        .rev()
        .find(|r| &r.session == session)
    {
        entry.workspace = workspace;
        orchestration.save(&state);
        return;
    }
    state.reviews_made += 1;
    let id = ReviewId(format!("r{}", state.reviews_made));
    state.reviews.push(Entry {
        id,
        project,
        session: session.clone(),
        mode,
        workspace,
        state: ReviewState::Open,
        summary: None,
        requested: false,
        rejected: None,
        decided: Vec::new(),
        decided_diffs: BTreeMap::new(),
    });
    orchestration.save(&state);
}

pub(super) async fn handle(core: &Core, request: ReviewRequest) -> Result<Response> {
    let done = |(item, conflicts, restored)| {
        Response::Review(ReviewResponse::Done {
            item,
            conflicts,
            restored,
        })
    };
    match request {
        ReviewRequest::List { project } => {
            core.project_store().get(&project)?;
            follow_up(core, &project).await;
            let entries: Vec<Entry> = core
                .orchestration_state()
                .lock()
                .reviews
                .iter()
                .rev()
                .filter(|r| r.project == project)
                .cloned()
                .collect();
            let mut items = Vec::new();
            for entry in entries {
                let item = item(core, &entry).await?;
                let shown = entry.requested || entry.state != ReviewState::Open;
                if shown || !item.files.is_empty() {
                    items.push(item);
                }
            }
            Ok(Response::Review(ReviewResponse::List(items)))
        }
        ReviewRequest::Diff { id, path } => {
            let entry = find(core, &id)?;
            let path = RelPath::parse(&path)?;
            if entry.state != ReviewState::Open {
                let patch = entry.decided_diffs.get(path.as_str()).cloned();
                let patch = patch.ok_or_else(|| {
                    not_found(format!(
                        "the diff of {path} as it was decided (only small changes keep theirs)"
                    ))
                })?;
                return Ok(Response::Review(ReviewResponse::Diff {
                    path: path.to_string(),
                    patch,
                }));
            }
            let ws = entry.workspace;
            let patch = core
                .worktrees()
                .patch(ws.dir, ws.base, vec![path.clone()])
                .await?;
            Ok(Response::Review(ReviewResponse::Diff {
                path: path.to_string(),
                patch,
            }))
        }
        ReviewRequest::Approve { id, commit } => {
            let (item, conflicts) = approve(core, &id, commit).await?;
            prune(core, &item.project).await;
            let what = if conflicts.is_empty() {
                "had its work approved by the person"
            } else {
                "had its work approved by the person, except files that conflict with the checkout"
            };
            super::tasks::tell_coordinator(core, &item.session, what);
            Ok(done((item, conflicts, Vec::new())))
        }
        ReviewRequest::Reject { id } => {
            let outcome = reject(core, &id).await?;
            prune(core, &outcome.0.project).await;
            super::tasks::tell_coordinator(
                core,
                &outcome.0.session,
                "had its work rejected by the person; its changes were discarded",
            );
            Ok(done(outcome))
        }
        ReviewRequest::UndoReject { id } => Ok(done(undo_reject(core, &id).await?)),
        ReviewRequest::Comment { id, text } => {
            comment(core, &id, text).await?;
            Ok(Response::Ok(OkResponse::Ok))
        }
    }
}

fn find(core: &Core, id: &ReviewId) -> Result<Entry> {
    core.orchestration_state()
        .lock()
        .reviews
        .iter()
        .find(|r| &r.id == id)
        .cloned()
        .ok_or_else(|| not_found(format!("review {}", id.0)))
}

/// The session's latest review item.
pub(super) fn of_session(core: &Core, session: &SessionId) -> Option<Entry> {
    core.orchestration_state()
        .lock()
        .reviews
        .iter()
        .rev()
        .find(|r| &r.session == session)
        .cloned()
}

/// Changes `id`'s entry and tells clients.
fn update(core: &Core, id: &ReviewId, f: impl FnOnce(&mut Entry)) -> Result<Entry> {
    let entry = {
        let orchestration = core.orchestration_state();
        let mut state = orchestration.lock();
        let entry = state
            .reviews
            .iter_mut()
            .find(|r| &r.id == id)
            .ok_or_else(|| not_found(format!("review {}", id.0)))?;
        f(entry);
        let entry = entry.clone();
        orchestration.save(&state);
        entry
    };
    core.emit(ReviewEvent::Changed {
        project: entry.project.clone(),
        id: id.clone(),
    });
    Ok(entry)
}

pub(super) async fn item(core: &Core, entry: &Entry) -> Result<ReviewItem> {
    Ok(ReviewItem {
        id: entry.id.clone(),
        project: entry.project.clone(),
        session: entry.session.clone(),
        mode: entry.mode,
        state: entry.state,
        summary: entry.summary.clone(),
        files: if entry.state == ReviewState::Open {
            files(core, entry).await?
        } else {
            entry.decided.clone()
        },
        can_undo: entry.mode == SessionMode::Direct
            && entry.state == ReviewState::Rejected
            && core.sessions().direct.can_undo(&entry.session),
    })
}

/// The entry's changed files as they are now.
async fn files(core: &Core, entry: &Entry) -> Result<Vec<FileChange>> {
    let ws = &entry.workspace;
    if entry.mode == SessionMode::Direct
        && let Ok(files) = core.sessions().direct.files(&entry.session).await
    {
        return Ok(files);
    }
    let deltas = core
        .worktrees()
        .changes(ws.dir.clone(), ws.base.clone())
        .await?;
    // Only a worker's files wait for the direct session. A direct item asking
    // this would recurse into itself through `direct_touched`.
    let touched = if entry.mode == SessionMode::Direct {
        BTreeSet::new()
    } else {
        direct_touched(core, &entry.project).await
    };
    Ok(deltas
        .into_iter()
        .map(|d| FileChange {
            blocked: touched.contains(&d.path),
            // Nobody but the worker writes its worktree. A direct session
            // no longer tracked (decided) vouches for nothing.
            by: if entry.mode == SessionMode::Direct {
                ChangedBy::Outside
            } else {
                ChangedBy::Agent
            },
            path: d.path,
            kind: d.kind,
            from: d.from,
            additions: d.additions,
            deletions: d.deletions,
        })
        .collect())
}

/// Files the direct session of `project` changed and nobody has approved or
/// rejected yet; workers' changes to them wait (ADR 0005).
async fn direct_touched(core: &Core, project: &ProjectId) -> BTreeSet<String> {
    let open: Vec<Entry> = core
        .orchestration_state()
        .lock()
        .reviews
        .iter()
        .filter(|r| {
            &r.project == project && r.mode == SessionMode::Direct && r.state == ReviewState::Open
        })
        .cloned()
        .collect();
    let mut touched = BTreeSet::new();
    for entry in open {
        for file in Box::pin(files(core, &entry)).await.unwrap_or_default() {
            if file.by != ChangedBy::Outside {
                touched.insert(file.path);
            }
        }
    }
    touched
}

fn check_open(entry: &Entry) -> Result<()> {
    match entry.state {
        ReviewState::Open => Ok(()),
        _ => Err(conflict(
            "the review is already decided; comment to reopen it",
        )),
    }
}

fn rel_paths(files: &[FileChange]) -> Result<Vec<RelPath>> {
    files.iter().map(|f| RelPath::parse(&f.path)).collect()
}

/// Applies a worker's files to the main checkout, except those the direct
/// session touched, which wait, and those the person changed since the
/// worker started, which conflict. Both are returned for the person.
async fn approve(
    core: &Core,
    id: &ReviewId,
    commit: Option<String>,
) -> Result<(ReviewItem, Vec<String>)> {
    let entry = find(core, id)?;
    check_open(&entry)?;
    // What gets applied is the worktree as it is now, so it must be the
    // work the person looked at: a session still at work could change it
    // between their reading and this click.
    let busy = core
        .sessions()
        .find(&entry.session, |s| {
            !s.status().is_ended() && *s.status() != AgentStatus::Idle
        })
        .unwrap_or(false);
    if busy {
        return Err(conflict(
            "the agent is still working on these changes; approve once it is idle or stopped",
        ));
    }
    if commit.as_deref().is_some_and(|m| m.trim().is_empty()) {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "the commit message is empty",
        ));
    }
    if entry.mode == SessionMode::Direct {
        let direct = &core.sessions().direct;
        if commit.is_none() && direct.files(&entry.session).await.is_err() {
            // Ovra no longer knows which files are the agent's (it
            // restarted since): keeping the changes needs no such knowledge.
            let decided = files(core, &entry).await?;
            let diffs = freeze(core, &entry, &decided).await;
            let entry = update(core, id, |e| {
                e.state = ReviewState::Approved;
                e.decided = decided;
                e.decided_diffs = diffs;
            })?;
            return Ok((item(core, &entry).await?, Vec::new()));
        }
        let decided = files(core, &entry).await?;
        let diffs = freeze(core, &entry, &decided).await;
        let outcome = direct.approve(&entry.session, commit).await?;
        let entry = update(core, id, |e| {
            e.state = ReviewState::Approved;
            e.decided = decided;
            e.decided_diffs = diffs;
            if let Some(base) = outcome.base {
                e.workspace.base = base;
            }
        })?;
        return Ok((item(core, &entry).await?, outcome.conflicts));
    }
    let root = core.root(&entry.project)?;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    let decided = files(core, &entry).await?;
    let diffs = freeze(core, &entry, &decided).await;
    let (waiting, ready): (Vec<FileChange>, Vec<FileChange>) =
        decided.iter().cloned().partition(|f| f.blocked);
    let mut conflicts = worktrees
        .apply(
            ws.dir.clone(),
            ws.base.clone(),
            root.clone(),
            rel_paths(&ready)?,
        )
        .await?;
    let applied: Vec<RelPath> = rel_paths(&ready)?
        .into_iter()
        .filter(|p| !conflicts.iter().any(|c| c == p.as_str()))
        .collect();
    if let Some(message) = commit
        && !applied.is_empty()
    {
        worktrees.commit(root, applied, message).await?;
    }
    conflicts.extend(waiting.into_iter().map(|f| f.path));
    // What the worker does from now on is a new review; files left for the
    // person are in the outcome and the decided record.
    let base = worktrees.snapshot(ws.dir).await?;
    let entry = update(core, id, |e| {
        e.state = ReviewState::Approved;
        e.workspace.base = base;
        e.decided = decided;
        e.decided_diffs = diffs;
    })?;
    Ok((item(core, &entry).await?, conflicts))
}

/// Puts a worker's worktree back to where it started, keeping the diff for
/// a later comment. Nothing in the main checkout changes.
async fn reject(core: &Core, id: &ReviewId) -> Result<(ReviewItem, Vec<String>, Vec<String>)> {
    let entry = find(core, id)?;
    check_open(&entry)?;
    if entry.mode == SessionMode::Direct {
        return reject_direct(core, &entry).await;
    }
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    let files = files(core, &entry).await?;
    let diffs = freeze(core, &entry, &files).await;
    let diff = worktrees
        .patch(ws.dir.clone(), ws.base.clone(), rel_paths(&files)?)
        .await?;
    worktrees
        .restore(ws.dir, ws.base, rel_paths(&files)?)
        .await?;
    let restored = files.iter().map(|f| f.path.clone()).collect();
    let entry = update(core, id, |e| {
        e.state = ReviewState::Rejected;
        e.rejected = Some(diff);
        e.decided = files;
        e.decided_diffs = diffs;
    })?;
    Ok((item(core, &entry).await?, Vec::new(), restored))
}

/// Restores a direct session's own files from its baseline (ADR 0005);
/// files someone else changed too stay and come back as conflicts.
async fn reject_direct(
    core: &Core,
    entry: &Entry,
) -> Result<(ReviewItem, Vec<String>, Vec<String>)> {
    let decided = files(core, entry).await?;
    let diffs = freeze(core, entry, &decided).await;
    let outcome = core.sessions().direct.reject(&entry.session).await?;
    let entry = update(core, &entry.id, |e| {
        e.state = ReviewState::Rejected;
        e.decided = decided;
        e.decided_diffs = diffs;
        e.rejected = Some(outcome.diff);
        if let Some(base) = outcome.base {
            e.workspace.base = base;
        }
    })?;
    Ok((
        item(core, &entry).await?,
        outcome.conflicts,
        outcome.restored,
    ))
}

/// Puts back what a direct session's last reject reverted; the item is open
/// again, as before the reject.
async fn undo_reject(core: &Core, id: &ReviewId) -> Result<(ReviewItem, Vec<String>, Vec<String>)> {
    let entry = find(core, id)?;
    if entry.mode != SessionMode::Direct || entry.state != ReviewState::Rejected {
        return Err(conflict("only a rejected direct session can be undone"));
    }
    let outcome = core.sessions().direct.undo_reject(&entry.session).await?;
    let entry = update(core, id, |e| {
        e.state = ReviewState::Open;
        e.rejected = None;
        e.decided = Vec::new();
        e.decided_diffs = BTreeMap::new();
        if let Some(base) = outcome.base {
            e.workspace.base = base;
        }
    })?;
    Ok((
        item(core, &entry).await?,
        outcome.conflicts,
        outcome.restored,
    ))
}

/// Reopens the item and sends `text` to its session, resuming it if it
/// ended. After a reject, the agent also gets the diff it lost.
async fn comment(core: &Core, id: &ReviewId, text: String) -> Result<()> {
    agents::check_text(&text)?;
    let entry = find(core, id)?;
    let message = match &entry.rejected {
        Some(diff) if entry.state == ReviewState::Rejected => with_rejected(&text, diff),
        _ => text,
    };
    // A decided item stays the record of its decision: what the session
    // changes next opens a new item (`follow_up`), so a rejected item never
    // comes back open with nothing in it.
    continue_session(core, &entry.session, message).await?;
    Ok(())
}

/// Sends `text` as the session's next message, resuming it if it ended.
pub(super) async fn continue_session(core: &Core, session: &SessionId, text: String) -> Result<()> {
    let ended = core.sessions().find(session, |s| s.status().is_ended())?;
    let request = if ended {
        AgentsRequest::Resume {
            session: session.clone(),
            prompt: text,
        }
    } else {
        AgentsRequest::Send {
            session: session.clone(),
            text,
        }
    };
    agents::handle_agents(&core.agents_ctx(), request).await?;
    Ok(())
}

fn with_rejected(text: &str, diff: &str) -> String {
    // The comment and the diff go as one message, which has a size limit.
    let room = agents::MAX_TEXT_BYTES.saturating_sub(text.len() + WRAPPER_BYTES);
    let mut cut = diff.len().min(room);
    while !diff.is_char_boundary(cut) {
        cut -= 1;
    }
    let more = if cut < diff.len() {
        "\n(diff cut short)"
    } else {
        ""
    };
    format!(
        "{text}\n\nThe person rejected your changes, and they were reverted. This was the rejected diff:\n```diff\n{}\n```{more}",
        &diff[..cut]
    )
}

/// `request_review`: the agent's summary, shown in the inbox.
pub(super) fn request(core: &Core, session: &SessionId, summary: String) -> Result<Entry> {
    let mut entry = of_session(core, session)
        .ok_or_else(|| conflict("this session has no changes to review"))?;
    if entry.state != ReviewState::Open {
        entry = open_after(core, &entry);
    }
    update(core, &entry.id, |e| {
        e.summary = Some(summary);
        e.requested = true;
    })
}

/// Opens a new item for what each session of `project` changed since its
/// latest item was decided, so new work is not hidden in a decided item.
/// Changes made only outside the agent open nothing; a reject that can
/// still be undone keeps its item.
async fn follow_up(core: &Core, project: &ProjectId) {
    let latest: Vec<Entry> = {
        let state = core.orchestration_state().lock();
        let mut seen = BTreeSet::new();
        state
            .reviews
            .iter()
            .rev()
            .filter(|r| &r.project == project && seen.insert(r.session.clone()))
            .filter(|r| r.state != ReviewState::Open)
            .cloned()
            .collect()
    };
    for entry in latest {
        let undoable = entry.state == ReviewState::Rejected
            && entry.mode == SessionMode::Direct
            && core.sessions().direct.can_undo(&entry.session);
        if undoable {
            continue;
        }
        let new = files(core, &entry).await.unwrap_or_default();
        if new.iter().any(|f| f.by != ChangedBy::Outside) {
            let opened = open_after(core, &entry);
            core.emit(ReviewEvent::Changed {
                project: opened.project.clone(),
                id: opened.id,
            });
        }
    }
}

/// A new open item for `decided`'s session, comparing from where the
/// decision left it.
fn open_after(core: &Core, decided: &Entry) -> Entry {
    let orchestration = core.orchestration_state();
    let mut state = orchestration.lock();
    state.reviews_made += 1;
    let entry = Entry {
        id: ReviewId(format!("r{}", state.reviews_made)),
        state: ReviewState::Open,
        summary: None,
        requested: false,
        rejected: None,
        decided: Vec::new(),
        decided_diffs: BTreeMap::new(),
        ..decided.clone()
    };
    state.reviews.push(entry.clone());
    orchestration.save(&state);
    entry
}

/// The diffs of `files` as they are now, to keep with the decision; none
/// for a change too big to keep (its file list is kept either way).
async fn freeze(core: &Core, entry: &Entry, files: &[FileChange]) -> BTreeMap<String, String> {
    let mut diffs = BTreeMap::new();
    if files.len() > KEPT_DIFF_FILES {
        return diffs;
    }
    let ws = &entry.workspace;
    let mut bytes = 0;
    for file in files {
        let Ok(path) = RelPath::parse(&file.path) else {
            continue;
        };
        let patch = core
            .worktrees()
            .patch(ws.dir.clone(), ws.base.clone(), vec![path])
            .await;
        let Ok(patch) = patch else { continue };
        bytes += patch.len();
        if bytes > KEPT_DIFF_BYTES {
            return BTreeMap::new();
        }
        diffs.insert(file.path.clone(), patch);
    }
    diffs
}

/// After a decision: tells the worktrees which snapshots of the project's
/// repository its reviews still compare against, so the others can go.
/// While direct mode still tracks a session of the project (its agent
/// runs, its attribution is still known, or a reject can be undone), it
/// needs snapshots of its own, and nothing is pruned.
async fn prune(core: &Core, project: &ProjectId) {
    let direct = &core.sessions().direct;
    if direct.writer(project).is_some() {
        return;
    }
    let entries: Vec<Entry> = core
        .orchestration_state()
        .lock()
        .reviews
        .iter()
        .filter(|r| &r.project == project)
        .cloned()
        .collect();
    for entry in entries.iter().filter(|e| e.mode == SessionMode::Direct) {
        if direct.can_undo(&entry.session) || direct.files(&entry.session).await.is_ok() {
            return;
        }
    }
    let keep: BTreeSet<String> = entries.into_iter().map(|e| e.workspace.base).collect();
    if let Ok(root) = core.root(project) {
        // Failing to prune only keeps more than needed.
        let _ = core
            .worktrees()
            .prune_snapshots(root, keep.into_iter().collect())
            .await;
    }
}
