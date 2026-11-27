//! A worker's worktree when the project moved on since its base
//! (`docs/specs/main-moved.md`): `ReviewItem::behind` warns before Keep,
//! and `review.update` brings the project's changes in.

use std::collections::BTreeSet;
use std::path::Path;

use super::review::{self, Entry};
use super::{Result, conflict};
use crate::Core;
use crate::path::RelPath;
use crate::protocol::agents::SessionMode;
use crate::protocol::review::{Behind, ReviewId, ReviewItem, ReviewState};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::Delta;

/// What the last updates brought into a worker's worktree since the agent
/// last heard from the person: the next comment tells it.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Updates {
    files: BTreeSet<String>,
    conflicts: BTreeSet<String>,
}

impl Updates {
    pub(super) fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Ovra's note to the agent; file names are anyone's words.
    pub(super) fn note(&self) -> String {
        let count = match self.files.len() {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        let mut note = format!(
            "Ovra: the person brought the project's latest changes into your worktree ({count}), so files may differ from what you last read; read them again before you change them."
        );
        if !self.conflicts.is_empty() {
            let list: Vec<&str> = self.conflicts.iter().map(String::as_str).collect();
            note.push_str(&format!(
                " Where your changes and the project's met, these files now hold git conflict markers (<<<<<<<, =======, >>>>>>>); resolve them:\n{}",
                crate::untrusted::untrusted("file names", &list.join("\n"))
            ));
        }
        note
    }
}

/// Every path a delta names, the old path of a rename too.
fn paths(deltas: &[Delta]) -> BTreeSet<&str> {
    deltas
        .iter()
        .flat_map(|d| std::iter::once(d.path.as_str()).chain(d.from.as_deref()))
        .collect()
}

/// Whether `entry` can be behind: an open worker item.
fn applies(entry: &Entry) -> bool {
    entry.mode == SessionMode::Isolated && entry.state == ReviewState::Open
}

/// Fills `item.behind` from `main`, a look of the project's checkout at
/// `root`. Leaves it `None` when that cannot be read.
pub(super) async fn fill(
    core: &Core,
    entry: &Entry,
    item: &mut ReviewItem,
    root: &Path,
    main: &str,
) {
    if !applies(entry) {
        return;
    }
    let Ok(moved) = core
        .worktrees()
        .diff(
            root.to_owned(),
            entry.workspace.base.clone(),
            main.to_owned(),
        )
        .await
    else {
        return;
    };
    if moved.is_empty() {
        return;
    }
    let changed = paths(&moved);
    let clashes = item
        .files
        .iter()
        .filter(|f| {
            changed.contains(f.path.as_str())
                || f.from.as_deref().is_some_and(|p| changed.contains(p))
        })
        .map(|f| f.path.clone())
        .collect();
    item.behind = Some(Behind {
        files: u32::try_from(moved.len()).unwrap_or(u32::MAX),
        clashes,
    });
}

/// Looks at the project's checkout once for all of `entries` that can be
/// behind, and fills their items.
pub(super) async fn fill_all(core: &Core, entries: &[Entry], items: &mut [ReviewItem]) {
    let Some(first) = entries.iter().find(|e| applies(e)) else {
        return;
    };
    let Ok(root) = core.root(&first.project) else {
        return;
    };
    let Ok(main) = core.worktrees().look(root.clone()).await else {
        return;
    };
    for (entry, item) in entries.iter().zip(items.iter_mut()) {
        fill(core, entry, item, &root, &main).await;
    }
}

/// What `review.update` did.
pub(super) struct Outcome {
    pub(super) item: ReviewItem,
    pub(super) files: u32,
    pub(super) conflicts: Vec<String>,
    pub(super) written: bool,
}

/// `review.update` that wrote nothing: the item as it is, against `main`.
async fn unchanged(
    core: &Core,
    entry: &Entry,
    root: &Path,
    main: &str,
    conflicts: Vec<String>,
) -> Result<Outcome> {
    let mut item = review::item(core, entry).await?;
    fill(core, entry, &mut item, root, main).await;
    Ok(Outcome {
        item,
        files: 0,
        conflicts,
        written: false,
    })
}

/// `review.update`.
pub(super) async fn update(core: &Core, id: &ReviewId, conflicts_ok: bool) -> Result<Outcome> {
    let entry = review::find(core, id)?;
    if entry.mode != SessionMode::Isolated {
        return Err(ProtocolError::new(
            ErrorCode::BadRequest,
            "only a worker's copy can be updated from the project",
        ));
    }
    if entry.state != ReviewState::Open {
        return Err(conflict(
            "only an open review can be updated from the project",
        ));
    }
    if review::busy(core, &entry.session) {
        return Err(conflict(
            "the agent is still working on these changes; update once it is idle or stopped",
        ));
    }
    // What came in would reach a pull request unseen: the next push picks
    // from the last pushed snapshot, which lacks it.
    if entry.pull_request.is_some() || entry.pending_push.is_some() || entry.agent_pushed.is_some()
    {
        return Err(conflict(
            "this work was pushed already; update its branch on GitHub instead",
        ));
    }
    let root = core.root(&entry.project)?;
    review::servers_stopped(core, &entry.session).await?;
    let ws = entry.workspace.clone();
    let worktrees = core.worktrees();
    let mine = worktrees.snapshot(ws.dir.clone()).await?;
    let main = worktrees.snapshot(root.clone()).await?;
    let came = worktrees
        .diff(root.clone(), ws.base.clone(), main.clone())
        .await?;
    if came.is_empty() {
        return unchanged(core, &entry, &root, &main, Vec::new()).await;
    }
    // The direct agent's changes are not decided yet: they must not become
    // the worker's base, out of sight in its diff.
    let touched = review::direct_touched(core, &entry.project).await;
    let held: Vec<&str> = paths(&came)
        .into_iter()
        .filter(|p| touched.contains(*p))
        .collect();
    if !held.is_empty() {
        return Err(conflict(format!(
            "the agent working in your project changed {} and that is not decided yet; keep or discard its review first",
            held.join(", ")
        )));
    }
    let merged = worktrees
        .merge(root.clone(), ws.base.clone(), mine.clone(), main.clone())
        .await?;
    if !merged.whole.is_empty() {
        return Err(conflict(format!(
            "{} changed on both sides and cannot be merged line by line (a binary file, or one side deleted it); copy the version you want by hand",
            merged.whole.join(", ")
        )));
    }
    if !merged.conflicts.is_empty() && !conflicts_ok {
        return unchanged(core, &entry, &root, &main, merged.conflicts).await;
    }
    let written = worktrees
        .diff(ws.dir.clone(), mine.clone(), merged.commit.clone())
        .await?;
    let written: Vec<RelPath> = paths(&written)
        .into_iter()
        .map(RelPath::parse)
        .collect::<Result<_>>()?;
    let skipped = worktrees
        .apply(
            merged.commit.clone(),
            mine.clone(),
            ws.dir.clone(),
            written.clone(),
        )
        .await?;
    let came: BTreeSet<String> = paths(&came).into_iter().map(str::to_owned).collect();
    let files = u32::try_from(came.len()).unwrap_or(u32::MAX);
    // Only if the item is still what this update read: an approve or
    // another update meanwhile owns the base now.
    let mut stale = false;
    let entry = if skipped.is_empty() {
        review::update(core, id, |e| {
            if e.state != ReviewState::Open || e.workspace.base != ws.base {
                stale = true;
                return;
            }
            e.workspace.base = main.clone();
            e.updates.files.extend(came);
            e.updates.conflicts.extend(merged.conflicts.iter().cloned());
        })?
    } else {
        entry
    };
    if stale || !skipped.is_empty() {
        // All or nothing: what was written goes back to the worker's version.
        let back: Vec<RelPath> = written
            .into_iter()
            .filter(|p| !skipped.iter().any(|s| s == p.as_str()))
            .collect();
        let left = worktrees
            .apply(mine, merged.commit.clone(), ws.dir.clone(), back)
            .await?;
        let mut why = if stale {
            "the review changed while it was being updated; look at it again".to_owned()
        } else {
            format!(
                "nothing was brought in: these files could not be written safely (the worker changed them meanwhile, or they are not plain files): {}",
                skipped.join(", ")
            )
        };
        if !left.is_empty() {
            why.push_str(&format!(
                ". These files changed again meanwhile and still hold the merged version: {}",
                left.join(", ")
            ));
        }
        return Err(conflict(why));
    }
    let mut item = review::item(core, &entry).await?;
    if let Ok(now) = worktrees.look(root.clone()).await {
        fill(core, &entry, &mut item, &root, &now).await;
    }
    Ok(Outcome {
        item,
        files,
        conflicts: merged.conflicts,
        written: true,
    })
}
