// Words the inbox uses for review items and their files.

import type { ChangeKind, FileChange, ReviewItem, ReviewState } from "../lib/protocol";

export const STATE_LABEL: Record<ReviewState, string> = {
  open: "Waiting for review",
  approved: "Approved",
  rejected: "Rejected",
};

const KIND_LABEL: Record<ChangeKind, string> = {
  added: "added",
  modified: "modified",
  deleted: "deleted",
  renamed: "renamed",
};

/** Markers shown next to a file; each one also reads out to screen readers. */
export function fileMarkers(file: FileChange): string[] {
  const markers: string[] = [];
  if (file.by === "outside") markers.push("Changed outside the agent");
  if (file.by === "both") markers.push("Also changed outside the agent");
  if (file.blocked) markers.push("Waits for the direct agent");
  return markers;
}

/** `src/a.ts, modified, +3 −1` and, for a rename, where it came from. */
export function fileSummary(file: FileChange): string {
  const what = file.kind === "renamed" && file.from ? `renamed from ${file.from}` : KIND_LABEL[file.kind];
  return `${what}, +${file.additions} −${file.deletions}`;
}

/** One line for the item list: how many files, and what needs attention. */
export function itemSummary(item: ReviewItem): string {
  const files = item.files.length === 1 ? "1 file" : `${item.files.length} files`;
  const outside = item.files.filter((f) => f.by !== "agent").length;
  const blocked = item.files.filter((f) => f.blocked).length;
  if (item.files.length > 0 && item.files.every((f) => f.by === "outside"))
    return `${files}, all changed outside the agent`;
  const parts = [files];
  if (outside > 0) parts.push(`${outside} changed outside the agent`);
  if (blocked > 0) parts.push(`${blocked} waiting for the direct agent`);
  return parts.join(", ");
}

/** What approve does for this kind of session (ADR 0005); merging branches is never automatic. */
export function approveHint(item: ReviewItem): string {
  if (item.mode === "direct") return "Approve keeps the changes in your checkout.";
  const blocked = item.files.some((f) => f.blocked);
  return blocked
    ? "Approve copies the worker's changes into your checkout, except files that wait for the direct agent. Merging its branch stays with you."
    : "Approve copies the worker's changes into your checkout. Merging its branch stays with you.";
}

/** What reject does; files changed outside the agent are never restored. */
export function rejectHint(item: ReviewItem): string {
  if (item.mode === "isolated") return "Reject discards the worker's changes. Your checkout is not touched.";
  return item.files.some((f) => f.by !== "agent")
    ? "Reject restores the agent's files from the start of the session. Changes made outside the agent stay as they are and are listed for you."
    : "Reject restores the agent's files from the start of the session.";
}

/** The commit message offered on approve: the first line of the agent's summary. */
export function defaultCommitMessage(item: ReviewItem): string {
  return item.summary?.split("\n")[0]?.trim() ?? "";
}
