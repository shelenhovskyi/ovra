// Words the inbox uses for review items and their files.

import type { ChangeKind, FileChange, ReviewCheck, ReviewItem, ReviewState } from "../lib/protocol";

export const STATE_LABEL: Record<ReviewState, string> = {
  open: "Waiting for review",
  approved: "Kept",
  partly_approved: "Kept, not all copied",
  rejected: "Discarded",
  published: "Pull request opened",
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
  if (top === ".worktreeinclude") return "ignored files copied into every new worktree, such as .env";
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

/**
 * Markers shown next to a file; each one also reads out to screen readers.
 * `keptFirst` names the worker whose version of this file of an open item
 * was already kept.
 */
export function fileMarkers(
  file: FileChange,
  shared = false,
  notCopied: ReviewState | null = null,
  keptFrom: string | null = null,
  keptFirst: string | null = null,
): string[] {
  const markers: string[] = [];
  if (file.protected) markers.push(PROTECTED_MARKER);
  if (notCopied) {
    const why = file.blocked
      ? "it waited for the direct agent"
      : file.protected
        ? "left out as protected, or your version changed"
        : keptFrom
          ? `you kept ${keptFrom}'s version`
          : "your version changed";
    markers.push(notCopied === "approved" ? `Not copied (${why}); marked done` : `Not copied: ${why}`);
  }
  // A direct agent's commands write files too: "outside" says only that its
  // file tools did not, not who did.
  if (file.by === "outside") markers.push("Not changed by the agent's file tools");
  if (file.by === "both") markers.push("Also changed outside the agent's file tools");
  if (file.blocked && !notCopied) markers.push("Waits for the direct agent");
  if (keptFirst) markers.push(`${keptFirst}'s version is already kept`);
  else if (shared) markers.push("Also changed in another open review");
  return markers;
}

/**
 * Files changed in `item` that a worker of another open item changed too.
 * Between workers' items, keeping one first decides what the other copies
 * into, so a file whose version another item already kept is left out:
 * that decision is made. A direct agent's files are in place already, but a
 * worker's version of one still waits for it (`blocked`), so the direct
 * agent's item marks every file a worker changed too. Empty unless `item`
 * is open.
 */
export function sharedFiles(item: ReviewItem, items: readonly ReviewItem[]): Set<string> {
  const shared = new Set<string>();
  if (item.state !== "open") return shared;
  const direct = item.mode !== "isolated";
  const kept = direct ? new Map<string, ReviewItem>() : keptFirst(item, items);
  const others = new Set<string>();
  for (const other of items) {
    if (other.id === item.id || other.state !== "open" || other.mode !== "isolated") continue;
    // A worker's file that waits for the direct agent says so itself.
    for (const f of other.files) if (f.by !== "outside" && (direct || !f.blocked)) others.add(f.path);
  }
  for (const f of item.files)
    if (f.by !== "outside" && !f.blocked && others.has(f.path) && !kept.has(f.path)) shared.add(f.path);
  return shared;
}

/**
 * For each file of an open worker's item whose version another worker's
 * item already copied into the project: that item. Keeping this one copies
 * the file only if it is the same.
 */
export function keptFirst(item: ReviewItem, items: readonly ReviewItem[]): Map<string, ReviewItem> {
  const kept = new Map<string, ReviewItem>();
  if (item.state !== "open" || item.mode !== "isolated") return kept;
  for (const f of item.files) {
    if (f.by === "outside") continue;
    const other = items.find(
      (o) =>
        o.id !== item.id &&
        o.mode === "isolated" &&
        (o.state === "approved" || o.state === "partly_approved") &&
        !o.not_copied.includes(f.path) &&
        o.files.some((g) => g.path === f.path && g.by !== "outside"),
    );
    if (other) kept.set(f.path, other);
  }
  return kept;
}

/** A worker's own name, without its task: `docs` of `docs · Fix login`. */
export function workerName(name: string): string {
  const at = name.indexOf(" · ");
  return at > 0 ? name.slice(0, at) : name;
}

/**
 * Said before Keep on an open item whose files other open items changed
 * too: who else wrote them, and that only the version kept first is
 * copied. Null without shared files.
 */
export function sharedHint(
  item: ReviewItem,
  items: readonly ReviewItem[],
  nameOf: (i: ReviewItem) => string,
): string | null {
  // The hint is about which version is copied: a direct agent's item copies nothing.
  if (item.mode !== "isolated") return null;
  const shared = sharedFiles(item, items);
  if (shared.size === 0) return null;
  const others = items.filter(
    (o) => o.id !== item.id && o.state === "open" && o.files.some((f) => f.by !== "outside" && shared.has(f.path)),
  );
  const who = others.map((o) => workerName(nameOf(o)));
  const names = who.length > 1 ? `${who.slice(0, -1).join(", ")} and ${who.at(-1)}` : (who[0] ?? "");
  const files = [...shared].join(", ");
  return `${names} also changed ${files}. Only the version you keep first is copied; a later one that differs is left for you to choose, and one that is the same counts as copied.`;
}

/**
 * For each file a worker's item did not copy because another worker's
 * version of it was already kept: that item. Only the person's own
 * changes since are "your version changed".
 */
export function keptElsewhere(item: ReviewItem, items: readonly ReviewItem[]): Map<string, ReviewItem> {
  const kept = new Map<string, ReviewItem>();
  if (item.mode !== "isolated") return kept;
  for (const path of item.not_copied) {
    const other = items.find(
      (o) =>
        o.id !== item.id &&
        o.mode === "isolated" &&
        (o.state === "approved" || o.state === "partly_approved") &&
        !o.not_copied.includes(path) &&
        o.files.some((f) => f.path === path && f.by !== "outside"),
    );
    if (other) kept.set(path, other);
  }
  return kept;
}

/** `src/a.ts, modified, +3 −1` and, for a rename, where it came from. */
export function fileSummary(file: FileChange): string {
  const what = file.kind === "renamed" && file.from ? `renamed from ${file.from}` : KIND_LABEL[file.kind];
  return `${what}, +${file.additions} −${file.deletions}`;
}

/**
 * One line for the item list: how many files, and what needs attention.
 * Protected files get their own tag; files shared with other reviews are
 * marked once, on the file.
 */
export function itemSummary(item: ReviewItem): string {
  if (item.files.length === 0 && item.git_changes.length > 0) return "No changed files";
  const files = item.files.length === 1 ? "1 file" : `${item.files.length} files`;
  const outside = item.files.filter((f) => f.by !== "agent").length;
  const blocked = item.files.filter((f) => f.blocked).length;
  const parts = [files];
  if (item.files.length > 0 && item.files.every((f) => f.by === "outside"))
    return [...parts, "none by the agent's file tools"].join(", ");
  if (outside > 0) parts.push(`${outside} changed outside the agent's file tools`);
  if (blocked > 0) parts.push(`${blocked} waiting for the direct agent`);
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
      `Git hooks or config changed: ${listed}. They can make git run programs. Keep changes and Discard leave them as they are: check each one yourself before you use git here.`,
    );
  }
  if (item.git_unchecked) parts.push("Ovra could not check git hooks and config for this item: look at them yourself.");
  return parts.length > 0 ? parts.join(" ") : null;
}

/** An item's title: the kind of session and its name, without saying "Worker" twice. */
export function itemTitle(item: ReviewItem, name: string): string {
  if (item.mode === "isolated") {
    // A task's worker is named with its task (`docs · Fix login`).
    const at = name.indexOf(" · ");
    if (at > 0) return `${name.slice(0, at)} (task: ${name.slice(at + 3)})`;
    return /^worker\b/i.test(name) ? name : `Worker: ${name}`;
  }
  return `Direct agent: ${name}`;
}

/** A list title: the worker's own name (its task heads the group), or the direct agent's. */
export function shortTitle(item: ReviewItem, name: string): string {
  return item.mode === "isolated" ? workerName(name) : name;
}

/** The task a worker's item belongs to (`Fix login` of `docs · Fix login`), or null. */
export function taskOf(item: ReviewItem, name: string): string | null {
  const at = name.indexOf(" · ");
  return item.mode === "isolated" && at > 0 ? name.slice(at + 3) : null;
}

/** What approve does for this kind of session (ADR 0005), in one line. */
export function approveHint(item: ReviewItem): string {
  if (item.mode === "direct") return "Keep changes leaves them in your project folder.";
  return item.files.some((f) => f.blocked)
    ? "Keep changes copies the worker's changes into your project folder, except files that wait for the direct agent."
    : "Keep changes copies the worker's changes into your project folder.";
}

/** The rest of what approve does, for the item's Details; null for a direct agent's. */
export function approveDetail(item: ReviewItem): string | null {
  return item.mode === "direct"
    ? null
    : "A file you changed yourself since the worker started is not copied; you will see which.";
}

/** A file's path, with where a rename came from. */
export function fileName(f: FileChange): string {
  return f.from ? `${f.path} (from ${f.from})` : f.path;
}

const names = (files: FileChange[]) => files.map(fileName).join(", ");

/**
 * Said beside Keep changes before it is pressed when the item changes protected
 * paths: each file with what it does, and what Keep changes and Discard do to it.
 * A worker's are left out unless the person includes them; a direct
 * session's are already in the project folder, and Discard restores only
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
    if (other.length === 0) return [...parts, "Keep changes leaves them; Discard restores them."].join(" ");
    parts.push("Keep changes leaves them.");
    if (mine.length > 0)
      parts.push(
        `Discard restores ${names(mine)}: the agent changed ${mine.length === 1 ? "it" : "them"} with its own file tools.`,
      );
    parts.push(
      `Discard leaves ${names(other)} as ${other.length === 1 ? "it is" : "they are"}: changed outside the agent's file tools (by you, another program or the agent's commands).`,
    );
    return parts.join(" ");
  }
  const waiting = files.filter((f) => f.blocked);
  const wait = waiting.length > 0 ? ` Waiting for the direct agent either way: ${names(waiting)}.` : "";
  return `These can run code or change agent rules: ${described}. Keep changes leaves them out of your project folder unless you include them.${wait}`;
}

/**
 * What a partly approved item left, and what the person can do about it.
 * `keptFrom` names, by path, the worker whose version was kept first.
 */
export function notCopiedHint(item: ReviewItem, keptFrom: ReadonlyMap<string, string> = new Map()): string {
  // `not_copied` names both paths of a rename.
  const paths = (f: FileChange) => (f.from ? [f.path, f.from] : [f.path]);
  const blocked = new Set(item.files.filter((f) => f.blocked).flatMap(paths));
  const guarded = new Set(item.files.filter((f) => f.protected).flatMap(paths));
  const waited = item.not_copied.filter((p) => blocked.has(p));
  const left = item.not_copied.filter((p) => !blocked.has(p) && guarded.has(p));
  const rest = item.not_copied.filter((p) => !blocked.has(p) && !guarded.has(p));
  const changed = rest.filter((p) => !keptFrom.has(p));
  const byWorker = new Map<string, string[]>();
  for (const p of rest) {
    const who = keptFrom.get(p);
    if (who) byWorker.set(who, [...(byWorker.get(who) ?? []), p]);
  }
  const why = [
    ...[...byWorker].map(([who, files]) => `because you kept ${who}'s different version first: ${files.join(", ")}`),
    changed.length > 0 ? `because your version changed since the worker started: ${changed.join(", ")}` : "",
    waited.length > 0 ? `because the direct agent had changed them too: ${waited.join(", ")}` : "",
    left.length > 0 ? `because they can run code or change agent rules: ${left.join(", ")}` : "",
  ].filter(Boolean);
  return `Not copied into your project folder, ${why.join("; and ")}. The worker's version of each is in its diff below. Replace yours with the worker's version (you can undo it), copy parts by hand, or keep yours and mark this done.`;
}

/** What reject does: only what the agent's own file tools changed is restored. */
export function rejectHint(item: ReviewItem): string {
  if (item.mode === "isolated")
    return "Discard removes the worker's changes from its own copy of the project. Your project folder is not touched.";
  const restores =
    "Discard restores only the files the agent changed with its own file tools, as they were at the start of the session.";
  return item.files.some((f) => f.by !== "agent")
    ? `${restores} Files changed any other way (by you, another program or the agent's commands) stay as they are and are listed for you.`
    : restores;
}

/** The commit message offered on approve: the first line of the agent's summary. */
export function defaultCommitMessage(item: ReviewItem): string {
  return item.summary?.split("\n")[0]?.trim() ?? "";
}

/** The quiet line for the project's check before review. */
export function checkText(check: ReviewCheck): string {
  switch (check.state) {
    case "running":
      return "Check running";
    case "passed":
      return "Check passed";
    case "failed":
      return check.detail ? `Check failed: ${check.detail}` : "Check failed";
    case "not_run":
      return check.detail ? `Check not run: ${check.detail}` : "Check not run";
  }
}

/**
 * A check's output as plain text. It comes from code an agent wrote, so
 * nothing in it may change how other lines look: colours and cursor moves,
 * other control characters (a carriage return could hide a line) and
 * direction overrides are dropped.
 */
export function plainLog(lines: string[]): string {
  // ESC [ ... final byte (CSI), and ESC ] ... BEL or ESC \ (OSC).
  // biome-ignore lint/suspicious/noControlCharactersInRegex: escapes are what it removes
  const escapes = /\u001b\[[0-?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)?/g;
  // biome-ignore lint/suspicious/noControlCharactersInRegex: control characters are what it removes
  const controls = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u200e\u200f\u202a-\u202e\u2066-\u2069]/g;
  return lines.map((line) => line.replace(escapes, "").replace(controls, "")).join("\n");
}
