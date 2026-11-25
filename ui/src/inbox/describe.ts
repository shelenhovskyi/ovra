// Words the inbox uses for review items and their files.

import type { ChangeKind, FileChange, ReviewItem, ReviewState } from "../lib/protocol";

export const STATE_LABEL: Record<ReviewState, string> = {
  open: "Waiting for review",
  approved: "Kept",
  partly_approved: "Kept, not all copied",
  rejected: "Rejected",
};

const KIND_LABEL: Record<ChangeKind, string> = {
  added: "added",
  modified: "modified",
  deleted: "deleted",
  renamed: "renamed",
};

/**
 * Marks a change to a protected path (docs/specs/security.md): approve
 * applies it only when the person includes it.
 */
export const PROTECTED_MARKER = "Can run code or change agent rules";

/** What a protected path does, by its first or last name; null for other paths. */
function reasonFor(path: string): string | null {
  const parts = path.toLowerCase().split("/").filter(Boolean);
  const top = parts[0] ?? "";
  const last = parts[parts.length - 1] ?? "";
  if (last === ".envrc") return "runs commands when you enter the folder with direnv";
  if (top === ".mcp.json") return "tools the agent can start";
  if (parts.includes(".claude")) return "the agent's settings and permissions";
  if (["claude.md", "claude.local.md", "agents.md", "gemini.md"].includes(last))
    return "instructions every agent reads";
  if ([".vscode", ".idea", ".devcontainer"].includes(top)) return "editor tasks and containers that can run code";
  if ([".zed", ".cursor", ".gemini"].includes(top)) return "another editor's or agent's settings and rules";
  if (top === ".husky") return "git hooks: run when you commit or push";
  if (last === ".gitattributes") return "tells git which programs to run on files";
  if (top === ".gitmodules") return "other repositories git fetches into this one";
  if (top === ".lfsconfig") return "where git downloads large files from";
  // The folder `core.hooksPath` names: usually `.githooks/` or `hooks/`.
  if (parts.slice(0, -1).some((p) => /hooks$/.test(p))) return "git hooks: run when you commit or push";
  return null;
}

/** One plain line on what a protected file does, for people who do not know these names. */
export function protectedReason(file: FileChange): string {
  return reasonFor(file.path) ?? (file.from ? reasonFor(file.from) : null) ?? "can run code or change agent rules";
}

/** Markers shown next to a file; each one also reads out to screen readers. */
export function fileMarkers(file: FileChange, shared = false, notCopied: ReviewState | null = null): string[] {
  const markers: string[] = [];
  if (file.protected) markers.push(PROTECTED_MARKER);
  if (notCopied) {
    const why = file.blocked
      ? "it waited for the direct agent"
      : file.protected
        ? "left out as protected, or your version changed"
        : "your version changed";
    markers.push(notCopied === "approved" ? `Not copied (${why}); marked done` : `Not copied: ${why}`);
  }
  // A direct agent's commands write files too: "outside" says only that its
  // file tools did not, not who did.
  if (file.by === "outside") markers.push("Not changed by the agent's file tools");
  if (file.by === "both") markers.push("Also changed outside the agent's file tools");
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

/** One line for the item list: how many files, and what needs attention. Protected files get their own tag. */
export function itemSummary(item: ReviewItem, shared = 0): string {
  if (item.files.length === 0 && item.git_changes.length > 0) return "No changed files";
  const files = item.files.length === 1 ? "1 file" : `${item.files.length} files`;
  const outside = item.files.filter((f) => f.by !== "agent").length;
  const blocked = item.files.filter((f) => f.blocked).length;
  const parts = [files];
  if (item.files.length > 0 && item.files.every((f) => f.by === "outside"))
    return [...parts, "none by the agent's file tools"].join(", ");
  if (outside > 0) parts.push(`${outside} changed outside the agent's file tools`);
  if (blocked > 0) parts.push(`${blocked} waiting for the direct agent`);
  if (shared > 0) parts.push(`${shared} also in another open review`);
  if (item.not_copied.length > 0)
    parts.push(`${item.not_copied.length} not copied${item.state === "approved" ? ", marked done" : ""}`);
  return parts.join(", ");
}

/** The list's tag for an item with protected files, so it stands out; null without any. */
export function protectedTag(item: ReviewItem): string | null {
  const count = item.files.filter((f) => f.protected).length;
  if (count === 0) return null;
  return `${count === 1 ? "1 file" : `${count} files`} can run code or change agent rules`;
}

/** The list's tag for an item whose git hooks or config changed, or could not be checked; null otherwise. */
export function gitTag(item: ReviewItem): string | null {
  const count = item.git_changes.length;
  if (count > 0) return `${count === 1 ? "1 git hook or config file" : `${count} git hooks or config files`} changed`;
  return item.git_unchecked ? "Git hooks and config not checked" : null;
}

/**
 * The warning on an item for git hooks and config that changed since its
 * session started, by anyone: no diff shows them, and neither approve nor
 * reject touches them. Null when there are none and they were checked.
 */
export function gitWarning(item: ReviewItem): string | null {
  const parts: string[] = [];
  if (item.git_changes.length > 0) {
    const listed = item.git_changes.map((c) => `${c.path} (${KIND_LABEL[c.kind]})`).join(", ");
    parts.push(
      `Git hooks and config changed since the session started, by the agent or anyone else: ${listed}. Git runs hooks by itself, and its config can make it run programs. Ovra shows no diff of these and neither applies nor undoes them: Keep changes and Reject leave them as they are. Check each one yourself before you next use git here.`,
    );
  }
  if (item.git_unchecked) parts.push("Ovra could not check git hooks and config for this item: look at them yourself.");
  return parts.length > 0 ? parts.join(" ") : null;
}

/** An item's title: the kind of session and its name, without saying "Worker" twice. */
export function itemTitle(item: ReviewItem, name: string): string {
  if (item.mode === "isolated") return /^worker\b/i.test(name) ? name : `Worker: ${name}`;
  return `Direct agent: ${name}`;
}

/** What approve does for this kind of session (ADR 0005), in plain words. */
export function approveHint(item: ReviewItem): string {
  if (item.mode === "direct") return "Keep changes leaves them in your project folder.";
  const copies = item.files.some((f) => f.blocked)
    ? "Keep changes copies the worker's changes into your project folder, except files that wait for the direct agent."
    : "Keep changes copies the worker's changes into your project folder.";
  return `${copies} A file you changed yourself since the worker started is not copied; you will see which.`;
}

/** A file's path, with where a rename came from. */
export function fileName(f: FileChange): string {
  return f.from ? `${f.path} (from ${f.from})` : f.path;
}

const names = (files: FileChange[]) => files.map(fileName).join(", ");

/**
 * Said beside Approve before it is pressed when the item changes protected
 * paths: each file with what it does, and what Approve and Reject do to it.
 * A worker's are left out unless the person includes them; a direct
 * session's are already in the project folder, and Reject restores only
 * those the agent changed with its own file tools. Null when it has none.
 */
export function protectedHint(item: ReviewItem): string | null {
  const files = item.files.filter((f) => f.protected);
  if (files.length === 0) return null;
  const described = files.map((f) => `${fileName(f)} (${protectedReason(f)})`).join(", ");
  if (item.mode === "direct") {
    const mine = files.filter((f) => f.by === "agent");
    const other = files.filter((f) => f.by !== "agent");
    const parts = [`These can run code or change agent rules, and are already in your project folder: ${described}.`];
    if (other.length === 0) return [...parts, "Keep changes leaves them; Reject restores them."].join(" ");
    parts.push("Keep changes leaves them.");
    if (mine.length > 0)
      parts.push(
        `Reject restores ${names(mine)}: the agent changed ${mine.length === 1 ? "it" : "them"} with its own file tools.`,
      );
    parts.push(
      `Reject leaves ${names(other)} as ${other.length === 1 ? "it is" : "they are"}: changed outside the agent's file tools (by you, another program or the agent's commands).`,
    );
    return parts.join(" ");
  }
  const waiting = files.filter((f) => f.blocked);
  const wait = waiting.length > 0 ? ` Waiting for the direct agent either way: ${names(waiting)}.` : "";
  return `These can run code or change agent rules: ${described}. Keep changes leaves them out of your project folder unless you include them.${wait}`;
}

/** What a partly approved item left, and what the person can do about it. */
export function notCopiedHint(item: ReviewItem): string {
  // `not_copied` names both paths of a rename.
  const paths = (f: FileChange) => (f.from ? [f.path, f.from] : [f.path]);
  const blocked = new Set(item.files.filter((f) => f.blocked).flatMap(paths));
  const guarded = new Set(item.files.filter((f) => f.protected).flatMap(paths));
  const waited = item.not_copied.filter((p) => blocked.has(p));
  const left = item.not_copied.filter((p) => !blocked.has(p) && guarded.has(p));
  const changed = item.not_copied.filter((p) => !blocked.has(p) && !guarded.has(p));
  const why = [
    changed.length > 0 ? `because your version changed since the worker started: ${changed.join(", ")}` : "",
    waited.length > 0 ? `because the direct agent had changed them too: ${waited.join(", ")}` : "",
    left.length > 0 ? `because they can run code or change agent rules: ${left.join(", ")}` : "",
  ].filter(Boolean);
  return `Not copied into your project folder, ${why.join("; and ")}. The worker's version of each is in its diff below. Replace yours with the worker's version (you can undo it), copy parts by hand, or keep yours and mark this done.`;
}

/** What reject does: only what the agent's own file tools changed is restored. */
export function rejectHint(item: ReviewItem): string {
  if (item.mode === "isolated")
    return "Reject discards the worker's changes from its own copy of the project. Your project folder is not touched.";
  const restores =
    "Reject restores only the files the agent changed with its own file tools, as they were at the start of the session.";
  return item.files.some((f) => f.by !== "agent")
    ? `${restores} Files changed any other way (by you, another program or the agent's commands) stay as they are and are listed for you.`
    : restores;
}

/** The commit message offered on approve: the first line of the agent's summary. */
export function defaultCommitMessage(item: ReviewItem): string {
  return item.summary?.split("\n")[0]?.trim() ?? "";
}
