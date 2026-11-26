//! `git.*`: the git panel and the editor's change markers and blame for the
//! project's main checkout (`docs/specs/git-panel.md`). Push, pull and fetch
//! are not here: they need the person's credentials and run in a terminal.
//!
//! Paths are relative to the project root. The panel refreshes on
//! `fs.git_changed`; there are no `git.*` events.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;
use super::{ErrorCode, ProtocolError};

/// The most files `git.status` lists; the rest are counted as `truncated`.
pub const MAX_STATUS_FILES: u32 = 5_000;
/// The most diff lines `git.diff` returns for one file.
pub const MAX_DIFF_LINES: u32 = 20_000;
/// The largest file `git.show` and `git.blame` read, in bytes.
pub const MAX_GIT_FILE: u64 = 4 * 1024 * 1024;
/// The most commits `git.log` returns.
pub const MAX_LOG: u32 = 200;
/// The most commits `git.pulled` returns.
pub const MAX_PULLED: u32 = 100;
/// The longest commit message body `git.pulled` returns, in bytes.
pub const MAX_PULLED_BODY: usize = 2 * 1024;
/// The most changed files `git.pulled` lists for one commit.
pub const MAX_PULLED_FILES: u32 = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum GitRequest {
    /// Branch, upstream and changed files. Answers `git.no_repo` when the
    /// project is not in a git repository.
    #[serde(rename = "git.status")]
    Status { project: ProjectId },
    /// `git init` in the project folder: no commit, no remote, nothing
    /// fetched. A person's action, not an agent tool. `conflict` when the
    /// folder is already in a git repository or has a `.git` of its own.
    /// Emits `fs.git_changed`.
    #[serde(rename = "git.init")]
    Init { project: ProjectId },
    /// Adds the working-tree version of `paths` (new, changed or deleted
    /// files) to the index.
    #[serde(rename = "git.stage")]
    Stage {
        project: ProjectId,
        paths: Vec<String>,
    },
    /// Puts `paths` in the index back as they are in HEAD; the working tree
    /// is not touched.
    #[serde(rename = "git.unstage")]
    Unstage {
        project: ProjectId,
        paths: Vec<String>,
    },
    /// Stages one hunk of `git.diff` (`staged: false`) of a tracked file, or
    /// with `unstage` takes one hunk of the staged diff (`staged: true`) out
    /// of the index. `conflict` when the file changed since that diff.
    #[serde(rename = "git.stage_hunk")]
    StageHunk {
        project: ProjectId,
        path: String,
        hunk: Hunk,
        #[serde(default)]
        unstage: bool,
    },
    /// Throws away the unstaged changes of `paths`: tracked files go back to
    /// their index version, untracked files are deleted. Staged changes
    /// stay. The working tree is snapshotted first; answers `git.discarded`
    /// with the snapshot for `git.undo_discard`.
    #[serde(rename = "git.discard")]
    Discard {
        project: ProjectId,
        paths: Vec<String>,
    },
    /// Puts `paths` back as they were in a `git.discard` snapshot.
    #[serde(rename = "git.undo_discard")]
    UndoDiscard {
        project: ProjectId,
        undo: String,
        paths: Vec<String>,
    },
    /// `git commit` of what is staged, with the person's own config and
    /// hooks, as in their terminal. With `amend`, an empty message keeps the
    /// last commit's message. `bad_request` for an empty message or nothing
    /// staged (unless `amend`); a failing hook answers `conflict`
    /// with its output, and so does a commit stopped by `git.cancel` or
    /// because it ran too long (a hook or gpg waiting for input).
    #[serde(rename = "git.commit")]
    Commit {
        project: ProjectId,
        message: String,
        #[serde(default)]
        amend: bool,
    },
    /// Local branches, most recently committed first.
    #[serde(rename = "git.branches")]
    Branches { project: ProjectId },
    /// Switches to a local branch, as `git switch` (never forced): refused
    /// with `conflict` when changes would be overwritten, and while an
    /// agent session writes the main checkout.
    #[serde(rename = "git.switch")]
    Switch { project: ProjectId, branch: String },
    /// Stops the `git.commit`, `git.switch` or switching `git.create_branch`
    /// running in the project, with
    /// the hooks and gpg it started; that request then answers `conflict`.
    /// `ok` also when nothing runs.
    #[serde(rename = "git.cancel")]
    Cancel { project: ProjectId },
    /// Creates a branch at HEAD; with `switch`, switches to it (allowed
    /// while a direct session runs: the files stay the same).
    /// `bad_request` for a name git does not accept, `conflict` if it exists.
    #[serde(rename = "git.create_branch")]
    CreateBranch {
        project: ProjectId,
        name: String,
        #[serde(default)]
        switch: bool,
    },
    /// The diff of one file: index to working tree, or with `staged`, HEAD
    /// to index. An untracked file diffs as all added.
    #[serde(rename = "git.diff")]
    Diff {
        project: ProjectId,
        path: String,
        #[serde(default)]
        staged: bool,
    },
    /// A file's text in HEAD or in the index, for side-by-side diffs and
    /// the editor's change markers.
    #[serde(rename = "git.show")]
    Show {
        project: ProjectId,
        path: String,
        rev: GitRev,
    },
    /// Who last changed each line of the file in the working tree.
    #[serde(rename = "git.blame")]
    Blame { project: ProjectId, path: String },
    /// The latest commits of HEAD, newest first; `limit` up to [`MAX_LOG`].
    #[serde(rename = "git.log")]
    Log {
        project: ProjectId,
        #[serde(default = "default_log_limit")]
        limit: u32,
    },
    /// The commits HEAD gained since the last pull, merge, rebase or reset
    /// (`ORIG_HEAD..HEAD`), newest first, at most [`MAX_PULLED`], with
    /// their messages and changed files. None when git has no `ORIG_HEAD`.
    /// Read-only; for the git panel's "Summarize what's new".
    #[serde(rename = "git.pulled")]
    Pulled { project: ProjectId },
}

fn default_log_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitRev {
    Head,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum GitResponse {
    #[serde(rename = "git.status")]
    Status(RepoStatus),
    /// `git.status` of a project that is not in a git repository.
    #[serde(rename = "git.no_repo")]
    NoRepo,
    #[serde(rename = "git.discarded")]
    Discarded {
        /// Pass to `git.undo_discard`.
        undo: String,
    },
    #[serde(rename = "git.committed")]
    Committed(Committed),
    #[serde(rename = "git.branches")]
    Branches { branches: Vec<BranchInfo> },
    #[serde(rename = "git.diff")]
    Diff(FileDiff),
    #[serde(rename = "git.file")]
    File(GitFileText),
    #[serde(rename = "git.blame")]
    Blame(Blame),
    #[serde(rename = "git.log")]
    Log { commits: Vec<CommitInfo> },
    #[serde(rename = "git.pulled")]
    Pulled(Pulled),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Pulled {
    /// Where HEAD was before (`ORIG_HEAD`); `None` when git has none.
    pub from: Option<String>,
    pub commits: Vec<CommitDetail>,
    /// Commits left out after [`MAX_PULLED`].
    pub truncated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetail {
    pub commit: CommitInfo,
    /// The message after its first line, trimmed, cut to
    /// [`MAX_PULLED_BODY`] bytes.
    pub body: String,
    /// At most [`MAX_PULLED_FILES`].
    pub files: Vec<CommitFile>,
    /// Files left out after [`MAX_PULLED_FILES`].
    pub more_files: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitFile {
    pub path: String,
    /// Lines added and removed; `None` for a binary file.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RepoStatus {
    /// The current branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The commit HEAD points at; `None` before the first commit.
    pub head: Option<String>,
    /// The branch's upstream, such as `origin/main`.
    pub upstream: Option<String>,
    /// Commits the branch has that its upstream lacks, and the reverse.
    pub ahead: u32,
    pub behind: u32,
    /// A merge, rebase or similar that git is in the middle of.
    pub operation: Option<GitOperation>,
    /// Changed, untracked and conflicted files, by path; at most
    /// [`MAX_STATUS_FILES`].
    pub files: Vec<StatusFile>,
    /// Files left out after [`MAX_STATUS_FILES`].
    pub truncated: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusFile {
    pub path: String,
    /// The old path of a staged rename or copy.
    pub from: Option<String>,
    /// HEAD to index.
    pub staged: Option<GitChange>,
    /// Index to working tree. A conflicted file has `conflicted` here and
    /// nothing staged; an untracked file has `untracked`.
    pub unstaged: Option<GitChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChange {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Committed {
    pub commit: CommitInfo,
    /// What git and the hooks printed, cut to 64 KiB.
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    /// The full commit id.
    pub id: String,
    pub author: String,
    /// Author time, seconds since the Unix epoch.
    pub time: i64,
    /// The first line of the message.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    /// HEAD is on this branch.
    pub current: bool,
    pub upstream: Option<String>,
    /// The branch's latest commit; `None` for a branch without commits.
    pub commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    /// The old path of a staged rename.
    pub from: Option<String>,
    /// Binary files have no hunks.
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    /// Lines were left out after [`MAX_DIFF_LINES`]; such a diff cannot be
    /// staged by hunk.
    pub truncated: bool,
}

/// One hunk of a unified diff with three lines of context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// From 1 (0 for an empty old side).
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Git's text after the second `@@`, such as a function name.
    pub header: String,
    /// The lines without line breaks (a CRLF file keeps the `\r`), each
    /// starting with ` `, `+` or `-`,
    /// or `\` for git's "No newline at end of file".
    pub lines: Vec<String>,
}

impl Hunk {
    /// Checks that the hunk is one hunk of one file, so it cannot carry
    /// another file's patch into the index: every line is context, an
    /// addition or a removal (or git's `\` note right after one), the
    /// counts match the header, and lines hold no line breaks (a CRLF
    /// file's lines keep one trailing `\r`).
    pub fn check(&self) -> Result<(), ProtocolError> {
        let bad = |why: &str| {
            Err(ProtocolError::new(
                ErrorCode::BadRequest,
                format!("not a hunk of one file: {why}"),
            ))
        };
        if self.lines.is_empty() || self.lines.len() > MAX_DIFF_LINES as usize {
            return bad("it has no lines or too many");
        }
        let (mut old, mut new) = (0u32, 0u32);
        let mut after_line = false;
        for line in &self.lines {
            let body = line.strip_suffix('\r').unwrap_or(line);
            if body.contains(['\n', '\r']) {
                return bad("a line contains a line break");
            }
            match line.as_bytes().first() {
                Some(b' ') => (old, new) = (old + 1, new + 1),
                Some(b'-') => old += 1,
                Some(b'+') => new += 1,
                Some(b'\\') if after_line => {
                    after_line = false;
                    continue;
                }
                _ => return bad("a line is not context, an addition or a removal"),
            }
            after_line = true;
        }
        if old != self.old_lines || new != self.new_lines {
            return bad("its line counts do not match its header");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFileText {
    /// `None` when the file is not in that revision, is binary or is
    /// larger than [`MAX_GIT_FILE`].
    pub text: Option<String>,
    /// The file is in that revision.
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Blame {
    /// Consecutive lines from one commit, in line order, covering the file.
    pub ranges: Vec<BlameRange>,
    /// The commits `ranges` name, by id.
    pub commits: BTreeMap<String, CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameRange {
    /// From 1.
    pub start: u32,
    pub lines: u32,
    /// `None` for lines not committed yet.
    pub commit: Option<String>,
}
