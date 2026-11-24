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
    /// Sends the comment to the same session, which continues; the item
    /// stays open. Answers `ok`.
    #[serde(rename = "review.comment")]
    Comment { id: ReviewId, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ReviewResponse {
    #[serde(rename = "review.list")]
    List(Vec<ReviewItem>),
    #[serde(rename = "review.diff")]
    Diff { path: String, patch: String },
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
