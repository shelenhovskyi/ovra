import type { SessionId, SessionMode } from "./agents";
import type { Hunk } from "./git";
import type { PullRequest } from "./pr";
import type { ProjectId } from "./projects";

export type ReviewId = string;

/**
 * `partly_approved`: approved, but some of a worker's files were not copied (`not_copied`); they wait for `review.resolve`.
 * `published`: a worker's changes were pushed as a pull request (`pull_request`); keep and discard stay available.
 */
export type ReviewState = "open" | "approved" | "partly_approved" | "rejected" | "published";

export type ChangeKind = "added" | "modified" | "deleted" | "renamed";

/** `outside`: the person, a formatter or git; never reverted automatically (ADR 0005). */
export type ChangedBy = "agent" | "outside" | "both";

export type FileChange = {
  /** Relative to the project root. */
  path: string;
  kind: ChangeKind;
  /** The old path of a rename. */
  from: string | null;
  by: ChangedBy;
  additions: number;
  deletions: number;
  /** A worker's file the direct session also changed: it waits for that session's review. */
  blocked: boolean;
  /** The file, or the old path of a rename, is a protected path (agent configuration, tool folders); approve applies it only with `include_protected`. */
  protected: boolean;
};

export type ReviewItem = {
  id: ReviewId;
  project: ProjectId;
  session: SessionId;
  mode: SessionMode;
  state: ReviewState;
  /** The agent's own summary from `request_review`. */
  summary: string | null;
  files: FileChange[];
  /** A direct session's last reject can be undone (`review.undo_reject`). */
  can_undo: boolean;
  /** A worker's files approve did not copy (including protected files not included); the worker's version stays in its diff. */
  not_copied: string[];
  /** The last `review.take` can be undone (`review.undo_take`). */
  can_undo_take: boolean;
  /** The last `review.discard_hunk` can be undone (`review.undo_discard`). */
  can_undo_discard: boolean;
  /**
   * The keep can be undone (`review.undo_keep`): it made no commit, and
   * neither the agent nor another keep in the project came after it.
   */
  can_undo_keep: boolean;
  /**
   * An open worker item's files as listed here; `review.approve` sends it
   * back and is refused if the worker's files changed since. For an open
   * direct item with protected files: those files, which an approve that
   * includes them sends back. Null otherwise.
   */
  fingerprint: string | null;
  /**
   * Files of the repository's git folder or hooks folder (hooks, config)
   * that changed since the session started. No diff shows them, and
   * approve and reject never touch them: the person checks them.
   */
  git_changes: GitFileChange[];
  /** Hooks and config could not be compared: `git_changes` may be incomplete. */
  git_unchecked: boolean;
  /** The pull request a worker's item was published as; absent without one. */
  pull_request?: PullRequest;
  /** The project's check, run when a worker asked for review or a person's own agent ended a turn with changes; absent without one. */
  check?: ReviewCheck;
  /**
   * An open worker item whose project changed since its base (`docs/specs/main-moved.md`).
   * Only `review.list` and `review.updated` fill it.
   */
  behind?: Behind;
};

/** How far the project moved on since a worker's base. */
export type Behind = {
  /** Files of the project, uncommitted ones too, that differ from the worker's base. */
  files: number;
  /** The worker's files among them: Keep would not copy these. */
  clashes: string[];
};

/** `not_run`: it could not run here (no sandbox for it, for example); `detail` says why. */
export type ReviewCheckState = "running" | "passed" | "failed" | "not_run";

/** One run of the project's check (`.ovra/check.json`); its output comes from `review.check_log`. */
export type ReviewCheck = {
  state: ReviewCheckState;
  /** The command as the person wrote it. */
  command: string;
  /** Why it failed, in words. */
  detail?: string;
  /** It judged files that changed since; `review.run_check` runs it again. */
  stale?: boolean;
};

/** What a line or file of the agent's diff does to the project's tests and checks. */
export type ReviewWarningKind =
  | "test_deleted"
  | "test_edited"
  | "assertion_removed"
  | "test_skipped"
  | "check_silenced"
  | "check_config";

/** One thing to look at closely; `line` from 1, in the base version with `old`. `text` comes from the agent: plain text only. */
export type ReviewWarning = {
  kind: ReviewWarningKind;
  path: string;
  line?: number;
  old?: boolean;
  text?: string;
};

/** A changed file of the git folder or hooks folder, such as `.git/hooks/pre-commit`. */
export type GitFileChange = { path: string; kind: ChangeKind };

/** The person's comment on one line: from 1, in the agent's version, or with `old` in the base (a removed line). */
export type LineComment = { path: string; line: number; old?: boolean; text: string };

export type ReviewRequest =
  | { method: "review.list"; params: { project: ProjectId } }
  | { method: "review.diff"; params: { id: ReviewId; path: string } }
  /**
   * With `commit`, commits the agent's files with that message. A worker's
   * item, and a direct item's `include_protected`, need the `fingerprint`
   * the person reviewed. Without
   * `include_protected`, a worker's protected files are not copied (they
   * wait in `not_copied`) and a direct item with protected files is refused.
   */
  | {
      method: "review.approve";
      params: { id: ReviewId; commit: string | null; fingerprint?: string | null; include_protected?: boolean };
    }
  | { method: "review.reject"; params: { id: ReviewId } }
  /** Puts back what the last reject reverted; files changed since come back as conflicts. */
  | { method: "review.undo_reject"; params: { id: ReviewId } }
  /** The person is done with what a partly approved item did not copy: it becomes approved. */
  | { method: "review.resolve"; params: { id: ReviewId } }
  /**
   * Puts the worker's version over the person's for what a partly approved
   * item left; files the worker changed since come back as conflicts.
   * Protected files are copied only with `include_protected`.
   */
  | { method: "review.take"; params: { id: ReviewId; include_protected?: boolean } }
  /** Puts back the person's versions from just before the last take. */
  | { method: "review.undo_take"; params: { id: ReviewId } }
  /**
   * Takes back the keep: the item is open again with the same changes. A
   * worker's copied files go back to the person's versions (one changed
   * since stays, as a conflict); a direct session's files stay on disk.
   */
  | { method: "review.undo_keep"; params: { id: ReviewId } }
  /**
   * Throws away one hunk of an open item's file, as `review.diff` showed it,
   * in the agent's workspace. An added or deleted file goes back as a whole.
   * Refused while the agent works, for a rename, for a direct item's file
   * changed only outside the agent, and when the file no longer holds it.
   */
  | { method: "review.discard_hunk"; params: { id: ReviewId; path: string; hunk: Hunk } }
  /** Puts back the file of the last discard, if nobody changed it since. */
  | { method: "review.undo_discard"; params: { id: ReviewId } }
  /**
   * The same session continues with the comment and the comments on lines,
   * as one message; `text` may be empty when `lines` is not. A decided item
   * stays decided.
   */
  | { method: "review.comment"; params: { id: ReviewId; text: string; lines?: LineComment[] } }
  /** The last lines of the item's check output. */
  | { method: "review.check_log"; params: { id: ReviewId } }
  /** Runs the project's check again for an open item (once its agent's turn ends). Answers `ok`. */
  | { method: "review.run_check"; params: { id: ReviewId } }
  /** What the agent's changes do to the project's tests and checks. */
  | { method: "review.warnings"; params: { id: ReviewId } }
  /**
   * Brings the project's changes since a worker's base into its worktree, merged with the worker's.
   * Conflicting files are written (with markers) only with `conflicts`.
   */
  | { method: "review.update"; params: { id: ReviewId; conflicts?: boolean } };

export type ReviewResponse =
  | { type: "review.list"; data: ReviewItem[] }
  /** For a worker's item, and a direct one with protected files, the item's fingerprint the diff was read with. */
  | { type: "review.diff"; data: { path: string; patch: string; fingerprint?: string | null } }
  /** After approve, reject or undo; `conflicts` are files left for the person, `restored` what a reject restored or an undo put back. */
  | { type: "review.done"; data: { item: ReviewItem; conflicts: string[]; restored: string[] } }
  /** Stdout and stderr merged, oldest first. */
  | { type: "review.check_log"; data: { lines: string[] } }
  | { type: "review.warnings"; data: { warnings: ReviewWarning[] } }
  /** After `review.update`: `files` that came in, `conflicts` written with markers when `written`, else why nothing was. */
  | { type: "review.updated"; data: { item: ReviewItem; files: number; conflicts: string[]; written: boolean } };

export type ReviewEvent =
  /** Re-fetch the list. */
  | { type: "review.changed"; data: { project: ProjectId; id: ReviewId } }
  /** An agent asked for review of `id`; follows its `review.changed`, for notifications. */
  | { type: "review.requested"; data: { project: ProjectId; id: ReviewId } };
