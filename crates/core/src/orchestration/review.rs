//! Review items: one per session with a workspace. `open` until a person
//! approves or rejects it; a comment reopens it and continues the session.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{Result, Workspace, conflict, not_found};
use crate::Core;
use crate::agents;
use crate::path::RelPath;
use crate::protected;
use crate::protocol::agents::{AgentStatus, MessageFrom, SessionId, SessionMode};
use crate::protocol::audit::{AuditEvent, ReviewAction};
use crate::protocol::projects::ProjectId;
use crate::protocol::review::{
    ChangeKind, ChangedBy, FileChange, GitFileChange, ReviewEvent, ReviewId, ReviewItem,
    ReviewRequest, ReviewResponse, ReviewState,
};
use crate::protocol::{ErrorCode, OkResponse, ProtocolError, Response};
use crate::services::{Delta, Worktrees};

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
    pub(super) mode: SessionMode,
    pub(super) workspace: Workspace,
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
    /// A worker's files approve did not copy.
    #[serde(default)]
    not_copied: Vec<String>,
    /// The last `review.take`, while it can be undone.
    #[serde(default)]
    taken: Option<Taken>,
    /// Hooks and config as they had changed when the person decided, kept
    /// with `decided`.
    #[serde(default)]
    decided_git: Option<GitSeen>,
}

/// What changed in the repository's hooks and config since a session
/// started ([`git_seen`]).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct GitSeen {
    changes: Vec<GitFileChange>,
    /// They could not be compared: the record or the reading failed.
    unchecked: bool,
    /// The hooks folder, relative to the project, if it is inside it.
    hooks_in_tree: Option<String>,
}

/// What a `review.take` did to the main checkout.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Taken {
    /// Snapshots of the checkout from just before and just after.
    before: String,
    after: String,
    /// The files it wrote.
    paths: Vec<String>,
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
        // A resumed session keeps what its hooks and config were when it
        // first started: what it changed since still shows.
        let git = entry.workspace.git.take().or(workspace.git);
        entry.workspace = Workspace { git, ..workspace };
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
        not_copied: Vec::new(),
        taken: None,
        decided: Vec::new(),
        decided_diffs: BTreeMap::new(),
        decided_git: None,
    });
    orchestration.save(&state);
}

/// Records the person's decision on `item` in the audit log.
fn audit(core: &Core, item: &ReviewItem, action: ReviewAction) {
    let event = AuditEvent::ReviewDecided {
        review: item.id.clone(),
        action,
    };
    crate::audit::record(core, &item.project, Some(&item.session), event);
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
                // A planted hook alone is a change to show.
                if shown || !item.files.is_empty() || !item.git_changes.is_empty() {
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
                    fingerprint: None,
                }));
            }
            let ws = entry.workspace.clone();
            let worktrees = core.worktrees();
            if entry.mode == SessionMode::Direct {
                // With protected files, read from the tree the item's
                // fingerprint names: the inbox can tell a newer diff.
                let (item, tree) = current(core, &entry).await?;
                if let (Some(fingerprint), Some(tree)) = (item.fingerprint, tree) {
                    let patch = worktrees
                        .patches(ws.dir, ws.base, tree, vec![path.clone()])
                        .await?
                        .remove(path.as_str())
                        .unwrap_or_default();
                    return Ok(Response::Review(ReviewResponse::Diff {
                        path: path.to_string(),
                        patch,
                        fingerprint: Some(fingerprint),
                    }));
                }
                let patch = worktrees.patch(ws.dir, ws.base, vec![path.clone()]).await?;
                return Ok(Response::Review(ReviewResponse::Diff {
                    path: path.to_string(),
                    patch,
                    fingerprint: None,
                }));
            }
            // Read with the item's fingerprint, from the same tree: the
            // inbox can tell a diff of newer files than it listed.
            let (_, fingerprint, tree) = listed(core, &entry, None).await?;
            let tree = tree.ok_or_else(|| conflict("the worker's files cannot be read"))?;
            let patch = worktrees
                .patches(ws.dir, ws.base, tree, vec![path.clone()])
                .await?
                .remove(path.as_str())
                .unwrap_or_default();
            Ok(Response::Review(ReviewResponse::Diff {
                path: path.to_string(),
                patch,
                fingerprint,
            }))
        }
        ReviewRequest::Approve {
            id,
            commit,
            fingerprint,
            include_protected,
        } => {
            let choice = Approval {
                commit,
                reviewed: fingerprint,
                include_protected,
            };
            let (item, conflicts) = approve(core, &id, choice).await?;
            let action = if item.state == ReviewState::PartlyApproved {
                ReviewAction::PartlyApproved
            } else {
                ReviewAction::Approved
            };
            audit(core, &item, action);
            prune(core, &item.project).await;
            super::tasks::clean_up(core, &item.project).await;
            let what = if item.state == ReviewState::PartlyApproved {
                "had its work approved by the person, except files the person had changed too, that wait for the direct session, or protected files the person did not include: those were not copied and wait for the person"
            } else if conflicts.is_empty() {
                "had its work approved by the person"
            } else {
                "had its work approved by the person; files they had changed too keep their version"
            };
            super::tasks::tell_coordinator(core, &item.session, what);
            Ok(done((item, conflicts, Vec::new())))
        }
        ReviewRequest::Reject { id } => {
            let outcome = reject(core, &id).await?;
            audit(core, &outcome.0, ReviewAction::Rejected);
            prune(core, &outcome.0.project).await;
            super::tasks::clean_up(core, &outcome.0.project).await;
            super::tasks::tell_coordinator(
                core,
                &outcome.0.session,
                "had its work rejected by the person; its changes were discarded",
            );
            Ok(done(outcome))
        }
        ReviewRequest::Resolve { id } => {
            let entry = find(core, &id)?;
            if entry.state != ReviewState::PartlyApproved && entry.taken.is_none() {
                return Err(conflict(
                    "only a partly approved review, or a replace that can be undone, waits to be resolved",
                ));
            }
            let entry = update(core, &id, |e| {
                e.state = ReviewState::Approved;
                e.taken = None;
            })?;
            super::tasks::clean_up(core, &entry.project).await;
            let resolved = item(core, &entry).await?;
            // The person settled what was left: the work counts as approved.
            audit(core, &resolved, ReviewAction::Approved);
            Ok(done((resolved, Vec::new(), Vec::new())))
        }
        ReviewRequest::Take {
            id,
            include_protected,
        } => {
            let outcome = take(core, &id, include_protected).await?;
            audit(core, &outcome.0, ReviewAction::Taken);
            prune(core, &outcome.0.project).await;
            super::tasks::clean_up(core, &outcome.0.project).await;
            Ok(done(outcome))
        }
        ReviewRequest::UndoTake { id } => {
            let outcome = undo_take(core, &id).await?;
            audit(core, &outcome.0, ReviewAction::UndoneTake);
            prune(core, &outcome.0.project).await;
            Ok(done(outcome))
        }
        ReviewRequest::UndoReject { id } => {
            let outcome = undo_reject(core, &id).await?;
            audit(core, &outcome.0, ReviewAction::UndoneReject);
            Ok(done(outcome))
        }
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

/// The base of `session`'s last decision, if nothing of its work waits for
/// the person any more: no item of it is partly approved, and its
/// workspace has no change since. Unknown means no.
pub(super) async fn settled(core: &Core, session: &SessionId) -> Option<String> {
    let (partial, latest) = {
        let state = core.orchestration_state();
        let state = state.lock();
        let mine = || state.reviews.iter().filter(|r| &r.session == session);
        // A replace that can still be undone keeps the worker's folder too.
        let partial = mine().any(|r| r.state == ReviewState::PartlyApproved || r.taken.is_some());
        (partial, mine().next_back().cloned())
    };
    let ws = latest.filter(|_| !partial)?.workspace;
    let changes = core.worktrees().changes(ws.dir, ws.base.clone()).await;
    changes.is_ok_and(|c| c.is_empty()).then_some(ws.base)
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
    Ok(current(core, entry).await?.0)
}

/// The entry's item, and for an open item the tree its files were read
/// from.
async fn current(core: &Core, entry: &Entry) -> Result<(ReviewItem, Option<String>)> {
    let (mut files, mut fingerprint, tree) = if entry.state != ReviewState::Open {
        (entry.decided.clone(), None, None)
    } else if super::tasks::cleaned(core, &entry.session) {
        // Removed only once nothing in it changed (`clean_up`).
        (Vec::new(), None, None)
    } else {
        listed(core, entry, None).await?
    };
    let git = match &entry.decided_git {
        Some(git) if entry.state != ReviewState::Open => git.clone(),
        _ => git_seen(core, entry).await,
    };
    mark_hooks(&mut files, git.hooks_in_tree.as_deref());
    if entry.mode == SessionMode::Direct
        && let Some(tree) = &tree
    {
        let ws = &entry.workspace;
        fingerprint =
            protected_fingerprint(core.worktrees(), &ws.dir, &ws.base, &files, tree).await;
    }
    let item = ReviewItem {
        id: entry.id.clone(),
        project: entry.project.clone(),
        session: entry.session.clone(),
        mode: entry.mode,
        state: entry.state,
        summary: entry.summary.clone(),
        files,
        can_undo: entry.mode == SessionMode::Direct
            && entry.state == ReviewState::Rejected
            && core.sessions().direct.can_undo(&entry.session),
        not_copied: entry.not_copied.clone(),
        can_undo_take: entry.taken.is_some(),
        fingerprint,
        git_changes: git.changes,
        git_unchecked: git.unchecked,
        pull_request: None,
    };
    Ok((item, tree))
}

/// Marks the files in the hooks folder git uses, when it is inside the
/// tree, as protected.
fn mark_hooks(files: &mut [FileChange], hooks_in_tree: Option<&str>) {
    let Some(hooks) = hooks_in_tree else { return };
    for file in files {
        let paths = std::iter::once(&file.path).chain(&file.from);
        file.protected |= paths.into_iter().any(|p| inside(p, hooks));
    }
}

/// What keeping a direct item's files needs: the person's choice for its
/// protected files, bound to the version they saw.
#[derive(Debug, Default)]
pub(super) struct Guard {
    /// The person chose to keep the protected files.
    pub(super) include_protected: bool,
    /// The `ReviewItem::fingerprint` the person reviewed.
    pub(super) reviewed: Option<String>,
    /// The hooks folder inside the tree, if git uses one (`GitSeen`).
    pub(super) hooks_in_tree: Option<String>,
    /// The base the item's fingerprint is read against.
    pub(super) base: String,
}

impl Guard {
    /// Marks `files`, read from `tree` of `dir`, and refuses them if a
    /// protected one is not included or not as the person saw it.
    pub(super) async fn admit(
        &self,
        worktrees: &dyn Worktrees,
        dir: &Path,
        files: &mut [FileChange],
        tree: Option<&String>,
    ) -> Result<()> {
        mark_hooks(files, self.hooks_in_tree.as_deref());
        if !self.include_protected {
            return super::direct::protected_refused(files);
        }
        if !files.iter().any(|f| f.protected) {
            return Ok(());
        }
        let now = match tree {
            Some(tree) => protected_fingerprint(worktrees, dir, &self.base, files, tree).await,
            None => None,
        };
        if now.is_none() || now != self.reviewed {
            return Err(conflict(
                "the protected files changed since you looked at them; review them again",
            ));
        }
        Ok(())
    }
}

/// A direct item's protected files as read from `tree`: SHA-256 over each
/// one's path, old path, kind and content version. `None` without
/// protected files, or when their versions cannot be read.
async fn protected_fingerprint(
    worktrees: &dyn Worktrees,
    dir: &Path,
    base: &str,
    files: &[FileChange],
    tree: &str,
) -> Option<String> {
    let guarded: Vec<&FileChange> = files.iter().filter(|f| f.protected).collect();
    if guarded.is_empty() {
        return None;
    }
    let deltas = worktrees
        .diff(dir.to_owned(), base.to_owned(), tree.to_owned())
        .await
        .ok()?;
    let mut hash = Sha256::new();
    hash.update(b"ovra-review-direct-v1\0");
    for f in guarded {
        // A file the diff does not show cannot be pinned to a version.
        let version = &deltas.iter().find(|d| d.path == f.path)?.version;
        let from = f.from.as_deref().unwrap_or("");
        for part in [f.path.as_str(), from, kind_name(f.kind), version.as_str()] {
            hash.update(part.as_bytes());
            hash.update(b"\0");
        }
    }
    Some(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// What changed in the project's hooks and config since the session
/// started. Read from the project, never through a worker's worktree,
/// whose `.git` file the worker could point elsewhere; hooks and config
/// are shared anyway.
async fn git_seen(core: &Core, entry: &Entry) -> GitSeen {
    let unchecked = GitSeen {
        unchecked: true,
        ..GitSeen::default()
    };
    let Some(seen) = &entry.workspace.git else {
        return unchecked;
    };
    let Ok(root) = core.root(&entry.project) else {
        return unchecked;
    };
    let Ok(now) = core.worktrees().git_files(root).await else {
        return unchecked;
    };
    let mut changes: Vec<GitFileChange> = now
        .files
        .iter()
        .filter_map(|(path, version)| {
            let kind = match seen.get(path) {
                None => ChangeKind::Added,
                Some(was) if was != version => ChangeKind::Modified,
                Some(_) => return None,
            };
            Some(GitFileChange {
                path: path.clone(),
                kind,
            })
        })
        .collect();
    changes.extend(
        seen.keys()
            .filter(|p| !now.files.contains_key(*p))
            .map(|path| GitFileChange {
                path: path.clone(),
                kind: ChangeKind::Deleted,
            }),
    );
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    GitSeen {
        changes,
        unchecked: false,
        hooks_in_tree: now.hooks_in_tree,
    }
}

/// Whether `path` is `folder` or inside it, ignoring case.
fn inside(path: &str, folder: &str) -> bool {
    let (path, folder) = (path.to_lowercase(), folder.to_lowercase());
    path == folder || path.starts_with(&format!("{folder}/"))
}

/// The entry's changed files as they are now.
async fn files(core: &Core, entry: &Entry) -> Result<Vec<FileChange>> {
    Ok(listed(core, entry, None).await?.0)
}

/// What [`listed`] found: the files, a worker's fingerprint of them
/// (`ReviewItem::fingerprint`), and the tree they were read from.
type Listed = (Vec<FileChange>, Option<String>, Option<String>);

/// The entry's changed files as they are in `at` (a snapshot of its
/// workspace), or else as they are now.
async fn listed(core: &Core, entry: &Entry, at: Option<&String>) -> Result<Listed> {
    let ws = &entry.workspace;
    if entry.mode == SessionMode::Direct
        && let Ok((files, tree)) = core.sessions().direct.files_at(&entry.session).await
    {
        return Ok((files, None, tree));
    }
    let worktrees = core.worktrees();
    // One reading of the workspace, so the files, their fingerprint and
    // whatever is copied or shown from `tree` are the same version.
    let tree = match at {
        Some(at) => at.clone(),
        None => worktrees.look(ws.dir.clone()).await?,
    };
    let mut deltas = worktrees
        .diff(ws.dir.clone(), ws.base.clone(), tree.clone())
        .await?;
    // A pod may still hold the sandbox's placeholders in a worker's
    // worktree: never a change to keep.
    deltas.retain(|d| !protected::is_placeholder(d));
    // Only a worker's files wait for the direct session. A direct item asking
    // this would recurse into itself through `direct_touched`.
    let touched = if entry.mode == SessionMode::Direct {
        BTreeSet::new()
    } else {
        direct_touched(core, &entry.project).await
    };
    let fingerprint =
        (entry.mode == SessionMode::Isolated).then(|| fingerprint(&ws.base, &deltas, &touched));
    let files = deltas
        .into_iter()
        .map(|d| FileChange {
            blocked: waits(&d, &touched),
            protected: protected::changes(&d.path, d.from.as_deref()),
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
        .collect();
    Ok((files, fingerprint, Some(tree)))
}

/// Whether a worker's change waits for the direct session: it touched the
/// file, or the old path of a rename.
fn waits(d: &Delta, touched: &BTreeSet<String>) -> bool {
    std::iter::once(&d.path)
        .chain(&d.from)
        .any(|p| touched.contains(p))
}

/// SHA-256 over the base and each changed file's path, old path, kind,
/// content version and whether it waits for the direct session: whatever
/// differs in what the person would see, or in what approve would copy,
/// changes it.
fn fingerprint(base: &str, deltas: &[Delta], waiting: &BTreeSet<String>) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ovra-review-v1\0");
    hash.update(base.as_bytes());
    hash.update(b"\0");
    for d in deltas {
        let from = d.from.as_deref().unwrap_or("");
        let waits = if waits(d, waiting) { "waits" } else { "" };
        for part in [
            d.path.as_str(),
            from,
            kind_name(d.kind),
            d.version.as_str(),
            waits,
        ] {
            hash.update(part.as_bytes());
            hash.update(b"\0");
        }
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn kind_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed => "renamed",
    }
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
                touched.extend(std::iter::once(file.path).chain(file.from));
            }
        }
    }
    touched
}

fn check_open(entry: &Entry) -> Result<()> {
    match entry.state {
        ReviewState::Open => Ok(()),
        _ => Err(conflict(
            "the review is already decided; a comment continues the agent, and its next changes open a new item",
        )),
    }
}

/// Every path of `files`, the old path of a rename too: approving a rename
/// deletes the old file, rejecting it brings the old file back.
fn rel_paths(files: &[FileChange]) -> Result<Vec<RelPath>> {
    let mut paths: Vec<&str> = files
        .iter()
        .flat_map(|f| std::iter::once(f.path.as_str()).chain(f.from.as_deref()))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths.into_iter().map(RelPath::parse).collect()
}

/// What the person chose in `review.approve`.
struct Approval {
    commit: Option<String>,
    /// The fingerprint of the files they looked at.
    reviewed: Option<String>,
    /// Apply protected files too.
    include_protected: bool,
}

/// Applies a worker's files to the main checkout, except those the direct
/// session touched, which wait, protected files the person did not
/// include, which wait too, and those the person changed since the worker
/// started, which conflict. All are returned for the person.
async fn approve(
    core: &Core,
    id: &ReviewId,
    choice: Approval,
) -> Result<(ReviewItem, Vec<String>)> {
    let Approval {
        commit,
        reviewed,
        include_protected,
    } = choice;
    let entry = find(core, id)?;
    check_open(&entry)?;
    // A session still at work would change the worktree while the person
    // reads it: its review would go stale under them. What lands is the
    // snapshot below either way.
    if busy(core, &entry.session) {
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
        // Protected files are already on disk: they are kept only as the
        // person saw them, and only by their choice. Each check reads the
        // same look of the folder as what it keeps.
        let git = git_seen(core, &entry).await;
        let guard = Guard {
            include_protected,
            reviewed,
            hooks_in_tree: git.hooks_in_tree.clone(),
            base: entry.workspace.base.clone(),
        };
        let direct = &core.sessions().direct;
        if commit.is_none() && direct.files(&entry.session).await.is_err() {
            // Ovra no longer knows which files are the agent's (it
            // restarted since): keeping the changes needs no such knowledge.
            let (mut decided, _, tree) = listed(core, &entry, None).await?;
            let ws = &entry.workspace;
            guard
                .admit(core.worktrees(), &ws.dir, &mut decided, tree.as_ref())
                .await?;
            let diffs = freeze(core, &entry, &decided, None).await;
            let entry = update(core, id, |e| {
                e.state = ReviewState::Approved;
                e.decided_git = Some(git);
                e.decided = decided;
                e.decided_diffs = diffs;
            })?;
            return Ok((item(core, &entry).await?, Vec::new()));
        }
        let outcome = direct.approve(&entry.session, commit, &guard).await?;
        let diffs = freeze(core, &entry, &outcome.files, outcome.seen.as_ref()).await;
        let entry = update(core, id, |e| {
            e.state = ReviewState::Approved;
            e.decided_git = Some(git);
            e.decided = outcome.files;
            e.decided_diffs = diffs;
            if let Some(base) = outcome.base {
                e.workspace.base = base;
            }
        })?;
        return Ok((item(core, &entry).await?, outcome.conflicts));
    }
    let root = core.root(&entry.project)?;
    servers_stopped(core, &entry.session).await?;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    // Only what the person reviewed may land: everything below reads and
    // copies this one snapshot, which must be what the inbox showed them,
    // whatever the worker writes meanwhile.
    let snapshot = worktrees.snapshot(ws.dir.clone()).await?;
    let (mut decided, now, _) = listed(core, &entry, Some(&snapshot)).await?;
    // Read first: the hooks folder in the tree is protected like the rest,
    // here and in the decided record `take` checks.
    let git = git_seen(core, &entry).await;
    mark_hooks(&mut decided, git.hooks_in_tree.as_deref());
    match reviewed {
        None => {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                "approving a worker's changes needs the fingerprint of the review you looked at",
            ));
        }
        Some(reviewed) if now.as_ref() != Some(&reviewed) => {
            return Err(conflict(
                "the worker's files changed since you looked at them; review them again",
            ));
        }
        Some(_) => {}
    }
    let mut diffs = freeze(core, &entry, &decided, Some(&snapshot)).await;
    // Protected files land only by the person's explicit choice; until then
    // they wait like files the direct session holds.
    let (waiting, ready): (Vec<FileChange>, Vec<FileChange>) = decided
        .iter()
        .cloned()
        .partition(|f| f.blocked || (f.protected && !include_protected));
    let mut conflicts = worktrees
        .apply(
            snapshot.clone(),
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
    // Both paths of a waiting rename: a later take then moves the file too.
    conflicts.extend(
        waiting
            .into_iter()
            .flat_map(|f| std::iter::once(f.path).chain(f.from)),
    );
    conflicts.sort();
    conflicts.dedup();
    // What was not copied lives on only in the worktree, so its diff is
    // kept whatever its size: it is the person's way to the worker's version.
    // A rename's diff is under its new path.
    let missing: Vec<RelPath> = conflicts
        .iter()
        .filter(|p| !diffs.contains_key(*p) && decided.iter().any(|f| &f.path == *p))
        .filter_map(|p| RelPath::parse(p).ok())
        .collect();
    if !missing.is_empty()
        && let Ok(patches) = worktrees
            .patches(ws.dir.clone(), ws.base.clone(), snapshot.clone(), missing)
            .await
    {
        diffs.extend(patches);
    }
    // What the worker does from now on is a new review; files left for the
    // person are in the outcome and the decided record. `take` copies them
    // from this snapshot: the version the person approved.
    let base = snapshot;
    let entry = update(core, id, |e| {
        e.decided_git = Some(git);
        // Approved only if everything the person read is now theirs.
        e.state = if conflicts.is_empty() {
            ReviewState::Approved
        } else {
            ReviewState::PartlyApproved
        };
        e.not_copied.clone_from(&conflicts);
        e.workspace.base = base;
        e.decided = decided;
        e.decided_diffs = diffs;
    })?;
    Ok((item(core, &entry).await?, conflicts))
}

/// Whether `session` is at work: neither idle nor ended.
/// Ends the servers of a worker's worktree before its files are copied:
/// they run commands that write it.
async fn servers_stopped(core: &Core, session: &SessionId) -> Result<()> {
    if crate::servers::worktree_stopped(core, session).await {
        return Ok(());
    }
    Err(conflict(
        "the worker's servers did not stop; try again once they have",
    ))
}

fn busy(core: &Core, session: &SessionId) -> bool {
    core.sessions()
        .find(session, |s| {
            !s.status().is_ended() && *s.status() != AgentStatus::Idle
        })
        .unwrap_or(false)
}

/// `review.take`: the worker's version over the person's, for what a partly
/// approved item left. Only what the person approved is copied: a file the
/// worker changed since, or that the direct agent has changed and nobody
/// decided yet, is skipped, and so is a protected file unless
/// `include_protected`: the person chose it, not the approve.
async fn take(
    core: &Core,
    id: &ReviewId,
    include_protected: bool,
) -> Result<(ReviewItem, Vec<String>, Vec<String>)> {
    let entry = find(core, id)?;
    if entry.state != ReviewState::PartlyApproved {
        return Err(conflict(
            "only a partly approved review has files to copy over yours",
        ));
    }
    if entry.taken.is_some() {
        return Err(conflict("keep or undo the last replace first"));
    }
    if busy(core, &entry.session) {
        return Err(conflict(
            "the worker is at work again; replace once it is idle or stopped",
        ));
    }
    let waits = direct_touched(core, &entry.project).await;
    let (_waiting, wanted): (Vec<String>, Vec<String>) = entry
        .not_copied
        .iter()
        .cloned()
        .partition(|p| waits.contains(p));
    // A protected file left by the approve may never have been looked at
    // (it waited for the direct agent then): only the person's explicit
    // choice copies it.
    let (guarded, wanted): (Vec<String>, Vec<String>) = wanted
        .into_iter()
        .partition(|p| !include_protected && guarded(&entry.decided, p));
    if wanted.is_empty() {
        return Err(if guarded.is_empty() {
            conflict("the files left wait for the direct agent; decide its review first")
        } else {
            ProtocolError::new(
                ErrorCode::BadRequest,
                format!(
                    "the files left are protected ({}); look at their diffs and replace with include_protected",
                    guarded.join(", ")
                ),
            )
        });
    }
    servers_stopped(core, &entry.session).await?;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    // Since the approve the base is the worktree as the person approved it.
    let since: BTreeSet<String> = worktrees
        .changes(ws.dir.clone(), ws.base.clone())
        .await?
        .into_iter()
        .flat_map(|d| std::iter::once(d.path).chain(d.from))
        .collect();
    let (changed, same): (Vec<String>, Vec<String>) =
        wanted.into_iter().partition(|p| since.contains(p));
    // Files that wait stay listed as not copied; they are not "skipped".
    let mut skipped = changed;
    let mut written = Vec::new();
    let mut taken = None;
    if !same.is_empty() {
        let root = core.root(&entry.project)?;
        let paths = same
            .iter()
            .map(|p| RelPath::parse(p))
            .collect::<Result<Vec<_>>>()?;
        // The base is the snapshot the person approved: its version, not
        // whatever is on the worker's disk now.
        let done = worktrees.take(ws.base.clone(), root.clone(), paths).await?;
        written = same
            .into_iter()
            .filter(|p| !done.skipped.contains(p))
            .collect();
        skipped.extend(done.skipped);
        if !written.is_empty() {
            // `after` is what the take wrote, so an undo leaves alone what
            // the person changed after it.
            taken = Some(Taken {
                before: done.undo,
                after: done.after,
                paths: written.clone(),
            });
        }
    }
    skipped.sort();
    let entry = update(core, id, |e| {
        e.not_copied.retain(|p| !written.contains(p));
        if e.not_copied.is_empty() {
            e.state = ReviewState::Approved;
        }
        e.taken = taken;
    })?;
    Ok((item(core, &entry).await?, skipped, written))
}

/// Whether `path`, left by an approve, is protected: one of the `decided`
/// files with it as its path or old path was marked so, or the path itself is.
fn guarded(decided: &[FileChange], path: &str) -> bool {
    protected::changes(path, None)
        || decided
            .iter()
            .any(|f| f.protected && (f.path == path || f.from.as_deref() == Some(path)))
}

/// `review.undo_take`: the person's versions from just before the last take.
/// A file changed since the take stays as it is and is listed as a conflict.
async fn undo_take(core: &Core, id: &ReviewId) -> Result<(ReviewItem, Vec<String>, Vec<String>)> {
    let entry = find(core, id)?;
    let Some(taken) = entry.taken.clone() else {
        return Err(conflict("there is no replace to undo"));
    };
    let root = core.root(&entry.project)?;
    let worktrees = core.worktrees();
    let since: BTreeSet<String> = worktrees
        .changes(root.clone(), taken.after.clone())
        .await?
        .into_iter()
        .map(|d| d.path)
        .collect();
    let (conflicts, back): (Vec<String>, Vec<String>) =
        taken.paths.into_iter().partition(|p| since.contains(p));
    if !back.is_empty() {
        let rel = back
            .iter()
            .map(|p| RelPath::parse(p))
            .collect::<Result<Vec<_>>>()?;
        worktrees.restore(root, taken.before, rel).await?;
    }
    let entry = update(core, id, |e| {
        for p in &back {
            if !e.not_copied.contains(p) {
                e.not_copied.push(p.clone());
            }
        }
        e.not_copied.sort();
        if !e.not_copied.is_empty() {
            e.state = ReviewState::PartlyApproved;
        }
        e.taken = None;
    })?;
    Ok((item(core, &entry).await?, conflicts, back))
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
    let diffs = freeze(core, &entry, &files, None).await;
    let diff = worktrees
        .patch(ws.dir.clone(), ws.base.clone(), rel_paths(&files)?)
        .await?;
    worktrees
        .restore(ws.dir, ws.base, rel_paths(&files)?)
        .await?;
    let restored = files.iter().map(|f| f.path.clone()).collect();
    let git = git_seen(core, &entry).await;
    let entry = update(core, id, |e| {
        e.state = ReviewState::Rejected;
        e.decided_git = Some(git);
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
    let outcome = core.sessions().direct.reject(&entry.session).await?;
    // From the snapshot taken just before the revert.
    let diffs = freeze(core, entry, &outcome.files, outcome.seen.as_ref()).await;
    let git = git_seen(core, entry).await;
    let entry = update(core, &entry.id, |e| {
        e.state = ReviewState::Rejected;
        e.decided_git = Some(git);
        e.decided = outcome.files;
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
        e.decided_git = None;
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
    // The person's words and Ovra's note of what was rejected go as one
    // message, each part shown with its author.
    let mut message = vec![(text, MessageFrom::Person)];
    if let Some(diff) = entry
        .rejected
        .as_ref()
        .filter(|_| entry.state == ReviewState::Rejected)
    {
        let note = rejected_note(message[0].0.len(), diff);
        message.push((note, MessageFrom::Ovra));
    }
    // A decided item stays the record of its decision: what the session
    // changes next opens a new item (`follow_up`), so a rejected item never
    // comes back open with nothing in it.
    continue_session(core, &entry.session, message).await?;
    Ok(())
}

/// Sends `text` as the session's next message, resuming it if it ended.
async fn continue_session(core: &Core, session: &SessionId, parts: agents::Parts) -> Result<()> {
    let ended = core.sessions().find(session, |s| s.status().is_ended())?;
    let ctx = core.agents_ctx();
    if ended {
        agents::resume(&ctx, session.clone(), parts).await?;
    } else {
        agents::send(&ctx, session.clone(), parts)?;
    }
    Ok(())
}

/// Ovra's note of a rejected `diff`, to follow a comment of `comment_len`
/// bytes in one message, which has a size limit.
fn rejected_note(comment_len: usize, diff: &str) -> String {
    let room = agents::MAX_TEXT_BYTES.saturating_sub(comment_len + WRAPPER_BYTES);
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
        "Ovra: the person rejected your changes, and they were reverted. This was the rejected diff:\n```diff\n{}\n```{more}",
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
        if undoable || super::tasks::cleaned(core, &entry.session) {
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
        not_copied: Vec::new(),
        taken: None,
        decided: Vec::new(),
        decided_diffs: BTreeMap::new(),
        decided_git: None,
        ..decided.clone()
    };
    // New work ends the undo of the decided item's last replace.
    let mut ended_undo = false;
    if let Some(old) = state.reviews.iter_mut().find(|r| r.id == decided.id) {
        ended_undo = old.taken.take().is_some();
    }
    state.reviews.push(entry.clone());
    orchestration.save(&state);
    drop(state);
    if ended_undo {
        core.emit(ReviewEvent::Changed {
            project: decided.project.clone(),
            id: decided.id.clone(),
        });
    }
    entry
}

/// The diffs of `files` as they are in `at` (a snapshot of the workspace)
/// or else now, to keep with the decision; none for a change too big to
/// keep (its file list is kept either way).
async fn freeze(
    core: &Core,
    entry: &Entry,
    files: &[FileChange],
    at: Option<&String>,
) -> BTreeMap<String, String> {
    let mut diffs = BTreeMap::new();
    if files.len() > KEPT_DIFF_FILES {
        return diffs;
    }
    let ws = &entry.workspace;
    let paths = files.iter().filter_map(|f| RelPath::parse(&f.path).ok());
    let worktrees = core.worktrees();
    let now = match at {
        Some(at) => Ok(at.clone()),
        None => worktrees.look(ws.dir.clone()).await,
    };
    let patches = match now {
        Ok(now) => {
            let paths = paths.collect();
            worktrees
                .patches(ws.dir.clone(), ws.base.clone(), now, paths)
                .await
        }
        Err(e) => Err(e),
    };
    let Ok(patches) = patches else {
        return diffs;
    };
    let mut bytes = 0;
    for file in files {
        let Some(patch) = patches.get(&file.path) else {
            continue;
        };
        bytes += patch.len();
        if bytes > KEPT_DIFF_BYTES {
            return BTreeMap::new();
        }
        diffs.insert(file.path.clone(), patch.clone());
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
    let keep: BTreeSet<String> = entries
        .into_iter()
        .flat_map(|e| {
            let taken = e.taken.map(|t| [t.before, t.after]).into_iter().flatten();
            std::iter::once(e.workspace.base).chain(taken)
        })
        .collect();
    if let Ok(root) = core.root(project) {
        // Failing to prune only keeps more than needed.
        let _ = core
            .worktrees()
            .prune_snapshots(root, keep.into_iter().collect())
            .await;
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn rename() -> Delta {
        Delta {
            path: "new.rs".into(),
            kind: ChangeKind::Renamed,
            from: Some("old.rs".into()),
            additions: 0,
            deletions: 0,
            version: "100644 abc".into(),
        }
    }

    #[test]
    fn the_fingerprint_names_the_base_and_what_waits() {
        let deltas = [rename()];
        let none = BTreeSet::new();
        let plain = fingerprint("base1", &deltas, &none);
        // The same changes over another base are another review.
        assert_ne!(plain, fingerprint("base2", &deltas, &none));
        // A rename waits when either of its paths does.
        let old = BTreeSet::from(["old.rs".to_owned()]);
        let new = BTreeSet::from(["new.rs".to_owned()]);
        assert_ne!(plain, fingerprint("base1", &deltas, &old));
        assert_eq!(
            fingerprint("base1", &deltas, &old),
            fingerprint("base1", &deltas, &new)
        );
    }
}
