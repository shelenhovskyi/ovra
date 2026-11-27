//! `review.*`: the inbox where a person approves or rejects agents' changes.

use serde::{Deserialize, Serialize};

use super::agents::{SessionId, SessionMode};
use super::git::Hunk;
use super::pr::PullRequest;
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
    /// A worker's changes were pushed as a pull request
    /// (`ReviewItem::pull_request`, `docs/specs/pr-flow.md`); keep and
    /// discard stay available.
    Published,
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
    /// run on their own. Approve applies it only when the person includes
    /// protected files (`review.approve`'s `include_protected`).
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
    /// changed since the worker started, the direct agent's changes to
    /// them wait, or they are protected and the person did not include
    /// them. The worker's version stays in its worktree and in
    /// `review.diff`.
    #[serde(default)]
    pub not_copied: Vec<String>,
    /// The last `review.take` can be undone (`review.undo_take`).
    #[serde(default)]
    pub can_undo_take: bool,
    /// The last `review.discard_hunk` can be undone (`review.undo_discard`).
    #[serde(default)]
    pub can_undo_discard: bool,
    /// The keep can be undone (`review.undo_keep`): it made no commit, and
    /// neither the agent nor another keep in the project came after it.
    #[serde(default)]
    pub can_undo_keep: bool,
    /// A worker's kept changes can still become a pull request
    /// (`pr.create`): its folder still holds them.
    #[serde(default)]
    pub can_publish_kept: bool,
    /// An open worker item's files as listed here (paths and contents);
    /// `review.approve` sends it back and is refused if the worker's files
    /// changed since. For an open direct item with protected files: those
    /// files (paths and contents), which an approve that includes them
    /// sends back, refused if they changed or others appeared. `None` for
    /// other direct items and decided ones.
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// Files of the repository's git folder or hooks folder, and other
    /// config git reads, that changed since the session started, by anyone
    /// (SEC-5): hooks, config. No diff shows them
    /// and approve and reject never touch them; the person checks them.
    #[serde(default)]
    pub git_changes: Vec<GitFileChange>,
    /// Hooks and config could not be compared (the record from the
    /// session's start or the reading now failed): `git_changes` may be
    /// incomplete, and the person should look.
    #[serde(default)]
    pub git_unchecked: bool,
    /// The pull request a worker's item was published as (`pr.create`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<Box<PullRequest>>,
    /// The project's check (`.ovra/check.json`) run in a worker's worktree
    /// when it asked for review, or in a person's own session's workspace
    /// when its turn ended with changes (`docs/specs/review-check.md`);
    /// `None` without a check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Box<ReviewCheck>>,
    /// Every repository of a worker's task across repositories, the main
    /// one first (`docs/specs/multi-repo.md`): its session has one item per
    /// repository, listed and kept together. Empty for one repository.
    #[serde(default, skip_serializing_if = "<[ProjectId]>::is_empty")]
    pub task_repos: Box<[ProjectId]>,
    /// An open worker item whose project changed since its base
    /// (`docs/specs/main-moved.md`); `None` when it did not, and for direct
    /// and decided items. Only `review.list` and `review.updated` fill it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<Behind>,
}

/// How far the project moved on since a worker's base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behind {
    /// Files of the project, uncommitted ones too, that differ from the
    /// worker's base.
    pub files: u32,
    /// The worker's files among them: Keep would not copy these.
    pub clashes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCheckState {
    Running,
    Passed,
    Failed,
    /// It could not run here (no sandbox for it, for example); `detail`
    /// says why.
    NotRun,
}

/// One run of the project's check, its output in `review.check_log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCheck {
    pub state: ReviewCheckState,
    /// The command as the person wrote it, joined with spaces.
    pub command: String,
    /// Why it failed, in words ("exit code 1", "timed out after 600 s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// It judged files that changed since: `review.run_check` runs it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
}

/// What a line or file of an item's diff does to the project's tests and
/// checks (`review.warnings`). Closed set: the inbox words each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewWarningKind {
    /// A test file was deleted.
    TestDeleted,
    /// Lines of an existing test file were removed or rewritten.
    TestEdited,
    /// An assertion was removed (`old` line).
    AssertionRemoved,
    /// A test was skipped, marked to ignore, or narrowed with `only`.
    TestSkipped,
    /// A lint, type or compiler check was silenced on a line.
    CheckSilenced,
    /// A file that sets up the project's checks changed: CI, lint or test
    /// runner config.
    CheckConfig,
}

/// One thing in an item's diff for the person to look at closely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewWarning {
    pub kind: ReviewWarningKind,
    /// Relative to the project root.
    pub path: String,
    /// From 1, in the agent's version; with `old`, in the base version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub old: bool,
    /// The line as the agent left it (or removed it), trimmed and cut. It
    /// comes from the agent: plain text only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// A changed file of the git folder or hooks folder, by the name
/// `Worktrees::git_files` gives it (`.git/hooks/pre-commit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFileChange {
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
        /// item needs it, and so does a direct item's `include_protected`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fingerprint: Option<String>,
        /// The person chose to apply the item's protected files too.
        /// Without it a worker's protected files are not copied (they wait
        /// in `not_copied`, as in a partial approve), and a direct item
        /// with protected files, already on disk, is refused.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        include_protected: bool,
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
    Take {
        id: ReviewId,
        /// The person chose to copy the protected files left too. Without
        /// it they stay not copied, and a take with nothing else left is
        /// refused.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        include_protected: bool,
    },
    /// Puts back the person's versions from just before the last
    /// `review.take`; the item is partly approved again.
    #[serde(rename = "review.undo_take")]
    UndoTake { id: ReviewId },
    /// Takes back the item's keep (`ReviewItem::can_undo_keep`): the item
    /// is open again with the same changes. For a worker, the files the
    /// keep copied into the project go back to the person's versions from
    /// just before; one changed since stays and is listed as a conflict.
    /// For a direct session nothing on disk changes: its files were the
    /// agent's all along, and discard can revert them again. Answers
    /// `review.done` with the files put back in `restored`.
    #[serde(rename = "review.undo_keep")]
    UndoKeep { id: ReviewId },
    /// Throws away one hunk of an open item's file, as `review.diff` showed
    /// it, in the agent's workspace: a worker's worktree, or the project
    /// for a direct session. An added or deleted file is one hunk: it goes
    /// back to the base as a whole. Refused while the agent works, for a
    /// rename, for a direct item's file changed only outside the agent, and
    /// when the file no longer holds the hunk. Answers `review.done` with
    /// the path in `restored`.
    #[serde(rename = "review.discard_hunk")]
    DiscardHunk {
        id: ReviewId,
        path: String,
        hunk: Hunk,
    },
    /// Puts back the file of the item's last `review.discard_hunk` as it was
    /// just before, if nobody changed it since; else answers `conflict`.
    #[serde(rename = "review.undo_discard")]
    UndoDiscard { id: ReviewId },
    /// Sends the comment, and the comments on `lines`, to the same session
    /// as one message, and it continues. `text` may be empty when `lines`
    /// is not. An open item stays open; a decided one stays decided, and
    /// the session's next changes open a new item. Answers `ok`.
    #[serde(rename = "review.comment")]
    Comment {
        id: ReviewId,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        lines: Vec<LineComment>,
    },
    /// The output of the item's check (`ReviewItem::check`), its last
    /// lines. Answers `review.check_log`.
    #[serde(rename = "review.check_log")]
    CheckLog { id: ReviewId },
    /// Runs the project's check again for an open item, now if its agent
    /// is not working, else once its turn ends. Answers `ok`; the item's
    /// `review.changed` follows.
    #[serde(rename = "review.run_check")]
    RunCheck { id: ReviewId },
    /// What the agent's changes in the item do to the project's tests and
    /// checks. Answers `review.warnings`.
    #[serde(rename = "review.warnings")]
    Warnings { id: ReviewId },
    /// Brings the project's changes since an open worker item's base into
    /// its worktree, merged with the worker's (`docs/specs/main-moved.md`);
    /// the base becomes the project as it is now. Refused while the worker
    /// works. When files conflict nothing is written, unless `conflicts`:
    /// then they are written with conflict markers. Answers
    /// `review.updated`.
    #[serde(rename = "review.update")]
    Update {
        id: ReviewId,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        conflicts: bool,
    },
}

/// The person's comment on one line of a file's diff (`review.comment`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineComment {
    /// Relative to the project root.
    pub path: String,
    /// From 1, in the agent's version of the file; with `old`, in the base
    /// version: a line the agent removed.
    pub line: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub old: bool,
    pub text: String,
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
        /// For a worker's item, and a direct item with protected files: the
        /// `ReviewItem::fingerprint` of the files this diff was read from.
        /// If it differs from the item's, the item changed since it was
        /// listed.
        #[serde(default)]
        fingerprint: Option<String>,
    },
    /// After approve, reject or undo: the item, and files left for the person.
    #[serde(rename = "review.done")]
    Done {
        item: Box<ReviewItem>,
        conflicts: Vec<String>,
        /// Files a reject restored, or an undo put back.
        #[serde(default)]
        restored: Vec<String>,
    },
    /// Stdout and stderr merged, oldest first, without line endings.
    #[serde(rename = "review.check_log")]
    CheckLog { lines: Vec<String> },
    /// In file order, then line order; at most a few per file.
    #[serde(rename = "review.warnings")]
    Warnings { warnings: Vec<ReviewWarning> },
    /// After `review.update`.
    #[serde(rename = "review.updated")]
    Updated {
        item: Box<ReviewItem>,
        /// Files of the project that came into the worktree (none when
        /// nothing was written).
        files: u32,
        /// Files both sides changed in the same place: written with
        /// conflict markers when `written`, else the reason nothing was.
        conflicts: Vec<String>,
        written: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ReviewEvent {
    /// An item was added, its files changed or its state changed; clients
    /// re-fetch the list.
    #[serde(rename = "review.changed")]
    Changed { project: ProjectId, id: ReviewId },
    /// An agent asked the person to review `id` (`request_review`). It
    /// follows the item's `review.changed`; it says only that work waits,
    /// for notifications.
    #[serde(rename = "review.requested")]
    Requested { project: ProjectId, id: ReviewId },
}
