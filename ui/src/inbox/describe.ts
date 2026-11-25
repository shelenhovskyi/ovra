// Words the inbox uses for review items and their files.

import type { ChangeKind, FileChange, ReviewItem, ReviewState } from "../lib/protocol";

export const STATE_LABEL: Record<ReviewState, string> = {
  open: "Waiting for review",
  approved: "Approved",
  partly_approved: "Approved, not all copied",
  rejected: "Rejected",
};

const KIND_LABEL: Record<ChangeKind, string> = {
  added: "added",
  modified: "modified",
  deleted: "deleted",
  renamed: "renamed",
};

/** Marks a change to a protected path (docs/specs/security.md): approve applies it like any other file. */
export const PROTECTED_MARKER = "Protected: agent or tool configuration";

/** Markers shown next to a file; each one also reads out to screen readers. */
export function fileMarkers(file: FileChange, shared = false, notCopied: ReviewState | null = null): string[] {
  const markers: string[] = [];
  if (file.protected) markers.push(PROTECTED_MARKER);
  if (notCopied) {
    const why = file.blocked ? "it waited for the direct agent" : "your version changed";
    markers.push(notCopied === "approved" ? `Not copied (${why}); marked done` : `Not copied: ${why}`);
  }
  if (file.by === "outside") markers.push("Changed outside the agent");
  if (file.by === "both") markers.push("Also changed outside the agent");
  if (file.blocked && !notCopied) markers.push("Waits for the direct agent");
  if (shared) markers.push("Also changed in another open review");
  return markers;
}

/**
 * Files an agent changed in `item` that an agent of another open item
 * changed too: approving one of them first decides what the other merges
 * into. Empty unless `item` is open.
 */
export function sharedFiles(item: ReviewItem, items: readonly ReviewItem[]): Set<string> {
  const shared = new Set<string>();
  if (item.state !== "open") return shared;
  const others = new Set<string>();
  for (const other of items) {
    if (other.id === item.id || other.state !== "open") continue;
    for (const f of other.files) if (f.by !== "outside") others.add(f.path);
  }
  for (const f of item.files) if (f.by !== "outside" && others.has(f.path)) shared.add(f.path);
  return shared;
}

/** `src/a.ts, modified, +3 −1` and, for a rename, where it came from. */
export function fileSummary(file: FileChange): string {
  const what = file.kind === "renamed" && file.from ? `renamed from ${file.from}` : KIND_LABEL[file.kind];
  return `${what}, +${file.additions} −${file.deletions}`;
}

/** One line for the item list: how many files, and what needs attention. */
export function itemSummary(item: ReviewItem, shared = 0): string {
  const files = item.files.length === 1 ? "1 file" : `${item.files.length} files`;
  const outside = item.files.filter((f) => f.by !== "agent").length;
  const blocked = item.files.filter((f) => f.blocked).length;
  const guarded = item.files.filter((f) => f.protected).length;
  const parts = [files];
  if (guarded > 0) parts.push(`${guarded} protected`);
  if (item.files.length > 0 && item.files.every((f) => f.by === "outside"))
    return [...parts, "all changed outside the agent"].join(", ");
  if (outside > 0) parts.push(`${outside} changed outside the agent`);
  if (blocked > 0) parts.push(`${blocked} waiting for the direct agent`);
  if (shared > 0) parts.push(`${shared} also in another open review`);
  if (item.not_copied.length > 0)
    parts.push(`${item.not_copied.length} not copied${item.state === "approved" ? ", marked done" : ""}`);
  return parts.join(", ");
}

/** What approve does for this kind of session (ADR 0005), in plain words. */
export function approveHint(item: ReviewItem): string {
  if (item.mode === "direct") return "Approve keeps the changes in your project folder.";
  const copies = item.files.some((f) => f.blocked)
    ? "Approve copies the worker's changes into your project folder, except files that wait for the direct agent."
    : "Approve copies the worker's changes into your project folder.";
  return `${copies} A file you changed yourself since the worker started is not copied; you will see which.`;
}

/**
 * Said beside Approve before it is pressed when the item changes protected
 * paths: approve applies them too, so the person checks them first. Null
 * when it changes none.
 */
export function protectedHint(item: ReviewItem): string | null {
  const paths = item.files.filter((f) => f.protected).map((f) => (f.from ? `${f.path} (from ${f.from})` : f.path));
  if (paths.length === 0) return null;
  return `Changes agent configuration or code that tools run on their own: ${paths.join(", ")}. Approve applies these too; check their diffs first.`;
}

/** What a partly approved item left, and what the person can do about it. */
export function notCopiedHint(item: ReviewItem): string {
  const blocked = new Set(item.files.filter((f) => f.blocked).map((f) => f.path));
  const changed = item.not_copied.filter((p) => !blocked.has(p));
  const waited = item.not_copied.filter((p) => blocked.has(p));
  const why = [
    changed.length > 0 ? `because your version changed since the worker started: ${changed.join(", ")}` : "",
    waited.length > 0 ? `because the direct agent had changed them too: ${waited.join(", ")}` : "",
  ].filter(Boolean);
  return `Not copied into your project folder, ${why.join("; and ")}. The worker's version of each is in its diff below. Replace yours with the worker's version (you can undo it), copy parts by hand, or keep yours and mark this done.`;
}

/** What reject does; files changed outside the agent are never restored. */
export function rejectHint(item: ReviewItem): string {
  if (item.mode === "isolated")
    return "Reject discards the worker's changes from its own copy of the project. Your project folder is not touched.";
  return item.files.some((f) => f.by !== "agent")
    ? "Reject restores the agent's files from the start of the session. Changes made outside the agent stay as they are and are listed for you."
    : "Reject restores the agent's files from the start of the session.";
}

/** The commit message offered on approve: the first line of the agent's summary. */
export function defaultCommitMessage(item: ReviewItem): string {
  return item.summary?.split("\n")[0]?.trim() ?? "";
}
