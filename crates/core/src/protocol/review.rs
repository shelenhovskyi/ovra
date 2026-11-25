//! `review.*`: the inbox where a person approves or rejects agents' changes.

use serde::{Deserialize, Serialize};

use super::agents::{SessionId, SessionMode};
use super::projects::ProjectId;

/// One review item per session that changed files.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReviewId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Open,
    Approved,
    /// Approved, but some of a worker's files were not copied into the
    /// project folder (`ReviewItem::not_copied`): they wait for the person
    /// until `review.resolve`.
    PartlyApproved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Who changed a file since the session's base (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangedBy {
    Agent,
    /// The person, a formatter, git: never reverted automatically.
    Outside,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Relative to the project root.
    pub path: String,
    pub kind: ChangeKind,
    /// The old path of a rename.
    pub from: Option<String>,
    pub by: ChangedBy,
    pub additions: u32,
    pub deletions: u32,
    /// Set for a worker's file that the direct session also changed: it
    /// waits until that session is approved or rejected.
    pub blocked: bool,
    /// The file, or the old path of a rename, is a protected path
    /// (`docs/specs/security.md`): agent configuration or code that tools
    /// run on their own. Approve applies it too, so the inbox says so.
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: ReviewId,
    pub project: ProjectId,
    pub session: SessionId,
    pub mode: SessionMode,
    pub state: ReviewState,
    /// The agent's own summary from `request_review`, if it gave one.
    pub summary: Option<String>,
    pub files: Vec<FileChange>,
    /// A direct session's last reject can be undone (`review.undo_reject`).
    #[serde(default)]
    pub can_undo: bool,
    /// A worker's files that approve did not copy: the person's version
    /// changed since the worker started, or the direct agent's changes to
    /// them wait. The worker's version stays in its worktree and in
    /// `review.diff`.
    #[serde(default)]
    pub not_copied: Vec<String>,
    /// The last `review.take` can be undone (`review.undo_take`).
    #[serde(default)]
    pub can_undo_take: bool,
    /// An open worker item's files as listed here (paths and contents);
    /// `review.approve` sends it back and is refused if the worker's files
    /// changed since. `None` for direct and decided items.
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// Files of the repository's git folder or hooks folder that changed
    /// since the session started (SEC-5): hooks, config. No diff shows them
    /// and approve and reject never touch them; the person checks them.
    #[serde(default)]
    pub git_changes: Vec<GitChange>,
}

/// A changed file of the git folder or hooks folder, by the name
/// `Worktrees::git_files` gives it (`.git/hooks/pre-commit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ReviewRequest {
    /// Review items of a project, newest first.
    #[serde(rename = "review.list")]
    List { project: ProjectId },
    /// The unified diff of one file of an item.
    #[serde(rename = "review.diff")]
    Diff { id: ReviewId, path: String },
    /// Keeps the changes; with `commit`, commits the agent's files with
    /// that message.
    #[serde(rename = "review.approve")]
    Approve {
        id: ReviewId,
        commit: Option<String>,
        /// The `ReviewItem::fingerprint` the person reviewed; a worker's
        /// item needs it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fingerprint: Option<String>,
    },
    /// Restores the agent's files from the base. Files changed outside the
    /// agent since are left alone and listed as conflicts.
    #[serde(rename = "review.reject")]
    Reject { id: ReviewId },
    /// Puts back what the last reject of a direct session reverted, from
    /// the snapshot taken just before it; files changed since stay and are
    /// listed as conflicts. The item is open again.
    #[serde(rename = "review.undo_reject")]
    UndoReject { id: ReviewId },
    /// The person is done with the files a partly approved item did not
    /// copy (copied them by hand or kept their own): the item becomes
    /// approved and keeps the list. Answers `review.done`.
    #[serde(rename = "review.resolve")]
    Resolve { id: ReviewId },
    /// Copies the worker's version over the person's for the files a partly
    /// approved item did not copy (not those that wait for the direct
    /// agent). A file the worker changed since the approve is skipped and
    /// listed as a conflict. When nothing is left the item is approved.
    #[serde(rename = "review.take")]
    Take { id: ReviewId },
    /// Puts back the person's versions from just before the last
    /// `review.take`; the item is partly approved again.
    #[serde(rename = "review.undo_take")]
    UndoTake { id: ReviewId },
    /// Sends the comment to the same session, which continues. An open item
    /// stays open; a decided one stays decided, and the session's next
    /// changes open a new item. Answers `ok`.
    #[serde(rename = "review.comment")]
    Comment { id: ReviewId, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ReviewResponse {
    #[serde(rename = "review.list")]
    List(Vec<ReviewItem>),
    #[serde(rename = "review.diff")]
    Diff {
        path: String,
        patch: String,
        /// For a worker's item: the `ReviewItem::fingerprint` of the files
        /// this diff was read from. If it differs from the item's, the item
        /// changed since it was listed.
        #[serde(default)]
        fingerprint: Option<String>,
    },
    /// After approve, reject or undo: the item, and files left for the person.
    #[serde(rename = "review.done")]
    Done {
        item: ReviewItem,
        conflicts: Vec<String>,
        /// Files a reject restored, or an undo put back.
        #[serde(default)]
        restored: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ReviewEvent {
    /// An item was added, its files changed or its state changed; clients
    /// re-fetch the list.
    #[serde(rename = "review.changed")]
    Changed { project: ProjectId, id: ReviewId },
}
