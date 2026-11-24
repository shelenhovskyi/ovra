import type { SessionId, SessionMode } from "./agents";
import type { ProjectId } from "./projects";

export type ReviewId = string;

export type ReviewState = "open" | "approved" | "rejected";

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
};

export type ReviewRequest =
  | { method: "review.list"; params: { project: ProjectId } }
  | { method: "review.diff"; params: { id: ReviewId; path: string } }
  /** With `commit`, commits the agent's files with that message. */
  | { method: "review.approve"; params: { id: ReviewId; commit: string | null } }
  | { method: "review.reject"; params: { id: ReviewId } }
  /** Puts back what the last reject reverted; files changed since come back as conflicts. */
  | { method: "review.undo_reject"; params: { id: ReviewId } }
  /** The same session continues with the comment; the item stays open. */
  | { method: "review.comment"; params: { id: ReviewId; text: string } };

export type ReviewResponse =
  | { type: "review.list"; data: ReviewItem[] }
  | { type: "review.diff"; data: { path: string; patch: string } }
  /** After approve, reject or undo; `conflicts` are files left for the person, `restored` what a reject restored or an undo put back. */
  | { type: "review.done"; data: { item: ReviewItem; conflicts: string[]; restored: string[] } };

/** Re-fetch the list. */
export type ReviewEvent = { type: "review.changed"; data: { project: ProjectId; id: ReviewId } };
