import type { SessionId, SessionMode } from "./agents";
import type { ProjectId } from "./projects";

export type ReviewId = string;

/** `partly_approved`: approved, but some of a worker's files were not copied (`not_copied`); they wait for `review.resolve`. */
export type ReviewState = "open" | "approved" | "partly_approved" | "rejected";

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
  /**
   * An open worker item's files as listed here; `review.approve` sends it
   * back and is refused if the worker's files changed since. Null otherwise.
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
};

/** A changed file of the git folder or hooks folder, such as `.git/hooks/pre-commit`. */
export type GitFileChange = { path: string; kind: ChangeKind };

export type ReviewRequest =
  | { method: "review.list"; params: { project: ProjectId } }
  | { method: "review.diff"; params: { id: ReviewId; path: string } }
  /**
   * With `commit`, commits the agent's files with that message. A worker's
   * item needs the `fingerprint` the person reviewed. Without
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
  /** Puts the worker's version over the person's for what a partly approved item left; files the worker changed since come back as conflicts. */
  | { method: "review.take"; params: { id: ReviewId } }
  /** Puts back the person's versions from just before the last take. */
  | { method: "review.undo_take"; params: { id: ReviewId } }
  /** The same session continues with the comment; a decided item stays decided. */
  | { method: "review.comment"; params: { id: ReviewId; text: string } };

export type ReviewResponse =
  | { type: "review.list"; data: ReviewItem[] }
  /** For a worker's item, `fingerprint` is the item's fingerprint the diff was read with. */
  | { type: "review.diff"; data: { path: string; patch: string; fingerprint?: string | null } }
  /** After approve, reject or undo; `conflicts` are files left for the person, `restored` what a reject restored or an undo put back. */
  | { type: "review.done"; data: { item: ReviewItem; conflicts: string[]; restored: string[] } };

/** Re-fetch the list. */
export type ReviewEvent = { type: "review.changed"; data: { project: ProjectId; id: ReviewId } };
