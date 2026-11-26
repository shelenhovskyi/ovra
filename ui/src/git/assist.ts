// Prompts for the git panel's agent actions and the reading of the agent's
// answers (docs/specs/git-agent-actions.md). Pure functions: the panel
// gathers the data, `assist.svelte.ts` runs the session.

import type { CommitInfo, FileDiff, GitOperation, Pulled, StatusFile } from "../lib/protocol";
import { CHANGE_LABEL } from "./model.svelte";

export type AssistAction = "commits" | "message" | "pulled" | "conflicts";

/** One commit the agent suggests: its message and the whole files it takes. */
export type PlannedCommit = { message: string; files: string[] };

/** The most bytes of diff text one prompt carries; the message limit is 64 KiB. */
const DIFF_BUDGET = 40 * 1024;
/** The most bytes of a whole prompt: under the core's 64 KiB message limit. */
export const MAX_PROMPT = 62 * 1024;
/** The most files listed by name in a prompt. */
const MAX_LISTED = 300;
/** The most bytes the commits of `pulledPrompt` take. */
const PULLED_BUDGET = 48 * 1024;
/** The most bytes of one file's diff. */
const FILE_BUDGET = 12 * 1024;
/** The most files whose diff is asked for; the rest are named only. */
export const MAX_DIFFED = 60;
const MAX_COMMITS = 20;
const MAX_MESSAGE = 4 * 1024;

/** File names whose contents never go into a prompt, only their name. */
const SECRET =
  /(^|\/)(\.env(\..*)?|\.envrc|\.npmrc|\.pypirc|\.netrc|\.git-credentials|\.dockerconfigjson|\.dockercfg|credentials(\.json)?|secrets?\.[^/]*|service-account[^/]*\.json|id_[a-z0-9]+(\.pub)?|[^/]*\.(pem|key|p12|pfx|keystore|jks|tfvars|tfstate))$/i;

/**
 * Strings that look like tokens (the audit log's list in docs/specs/security.md):
 * a diff with one goes in by name only.
 */
const TOKEN =
  /\b(sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[a-z]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.)/;

export function looksSecret(path: string): boolean {
  return SECRET.test(path);
}

const encoder = new TextEncoder();

/** The UTF-8 size of `text`, as the core counts a message. */
export function bytes(text: string): number {
  return encoder.encode(text).length;
}

/** `text` cut to at most `max` UTF-8 bytes, on a character, with a note when cut. */
export function fitPrompt(text: string, max = MAX_PROMPT): string {
  if (bytes(text) <= max) return text;
  const note = "\n\n(The rest was cut to fit.)";
  const room = max - bytes(note);
  // The longest prefix that fits: UTF-16 units never take fewer bytes than one each.
  let low = 0;
  let high = Math.min(text.length, room);
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (bytes(text.slice(0, mid)) <= room) low = mid;
    else high = mid - 1;
  }
  // Not in the middle of a surrogate pair.
  const code = text.charCodeAt(low - 1);
  if (low > 0 && code >= 0xd800 && code <= 0xdbff) low--;
  return text.slice(0, low) + note;
}

/** One line per file: "src/a.ts (modified, staged)". */
function describe(file: StatusFile): string {
  const sides = [
    file.staged ? `${CHANGE_LABEL[file.staged]}, staged` : null,
    file.unstaged && file.unstaged !== "untracked" ? `${CHANGE_LABEL[file.unstaged]}, not staged` : null,
    file.unstaged === "untracked" ? "new, not staged" : null,
  ].filter(Boolean);
  const from = file.from ? ` (renamed from ${file.from})` : "";
  return `- ${file.path}${from} (${sides.join("; ")})`;
}

/** A diff as unified text, cut to `budget` bytes on a line. */
function diffText(diff: FileDiff, budget: number): { text: string; cut: boolean } {
  if (diff.binary) return { text: `--- ${diff.path}: binary file`, cut: false };
  const lines = [`--- ${diff.path}`];
  let size = bytes(lines[0] ?? "");
  for (const hunk of diff.hunks) {
    for (const line of [
      `@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@ ${hunk.header}`,
      ...hunk.lines,
    ]) {
      const length = bytes(line) + 1;
      if (size + length > budget) return { text: lines.join("\n"), cut: true };
      lines.push(line);
      size += length;
    }
  }
  return { text: lines.join("\n"), cut: diff.truncated };
}

/**
 * The diffs section: each file's diffs (staged and unstaged) within the
 * budgets, secrets named without content, cut files said so.
 */
export function diffsSection(diffs: readonly FileDiff[], budget = DIFF_BUDGET): string {
  const parts: string[] = [];
  const cut: string[] = [];
  const hidden: string[] = [];
  let left = budget;
  for (const diff of diffs) {
    if (looksSecret(diff.path) || (diff.from !== null && looksSecret(diff.from))) {
      if (!hidden.includes(diff.path)) hidden.push(diff.path);
      continue;
    }
    const { text, cut: wasCut } = diffText(diff, Math.min(FILE_BUDGET, left));
    if (TOKEN.test(text)) {
      if (!hidden.includes(diff.path)) hidden.push(diff.path);
      continue;
    }
    const header = `--- ${diff.path}`.length;
    if (text.length > header) {
      parts.push(text);
      left -= bytes(text) + 1;
    }
    if (wasCut || text.length <= header) cut.push(diff.path);
    if (left < 256) break;
  }
  const notes = [
    cut.length ? `Cut or left out (read these files yourself if you need more): ${cut.join(", ")}` : "",
    hidden.length ? `Contents not sent because they may hold a secret: ${hidden.join(", ")}` : "",
  ].filter(Boolean);
  return [...parts, ...notes].join("\n\n");
}

const ONLY_PROPOSE =
  "Do not run commands, edit files or use git yourself: you only propose, and the person commits from Ovra's git panel. " +
  "Never push. You may read files in the project if a diff is cut.";

function recent(log: readonly CommitInfo[]): string {
  return log.length ? log.map((c) => `- ${c.summary}`).join("\n") : "(none yet)";
}

/** The files, one per line, at most `MAX_LISTED`, then how many more. */
function listed(files: readonly StatusFile[]): string {
  const lines = files.slice(0, MAX_LISTED).map(describe);
  if (files.length > MAX_LISTED) lines.push(`- and ${count(files.length - MAX_LISTED, "more file")}`);
  return lines.join("\n");
}

function count(n: number, one: string): string {
  return `${n} ${one}${n === 1 ? "" : "s"}`;
}

/** Asks for commits of whole files, with messages, as one json block. */
export function commitsPrompt(
  files: readonly StatusFile[],
  diffs: readonly FileDiff[],
  log: readonly CommitInfo[],
): string {
  return [
    `Suggest commits for ${count(files.length, "changed file")}`,
    "Split the changes below into a few logical commits for this repository, and write a commit message for each " +
      "in the style of the recent commits (for example Conventional Commits, if they use them). " +
      "Group whole files only. Order the commits so each one makes sense on its own. " +
      ONLY_PROPOSE,
    "Answer with one short sentence, then exactly one ```json block, nothing after it:\n" +
      '```json\n{"commits": [{"message": "subject line\\n\\noptional body", "files": ["path/one", "path/two"]}]}\n```\n' +
      "Use only paths from the list of changed files, each at most once. Leave out files that should not be committed.",
    `Recent commits:\n${recent(log)}`,
    `Changed files:\n${listed(files)}`,
    `Diffs:\n${diffsSection(diffs)}`,
  ].join("\n\n");
}

/** Asks for one message for what is staged. */
export function messagePrompt(
  staged: readonly StatusFile[],
  diffs: readonly FileDiff[],
  log: readonly CommitInfo[],
): string {
  return [
    `Write a commit message for ${count(staged.length, "staged file")}`,
    "Write one commit message for the staged changes below, in the style of the recent commits " +
      "(for example Conventional Commits, if they use them): a subject line of at most 72 characters, " +
      "and a short body only when the why is not obvious. " +
      ONLY_PROPOSE,
    "Answer with only the message in one ```text block.",
    `Recent commits:\n${recent(log)}`,
    `Staged files:\n${listed(staged)}`,
    `Staged diffs:\n${diffsSection(diffs)}`,
  ].join("\n\n");
}

/** Asks for a short summary of the commits a pull brought. */
export function pulledPrompt(pulled: Pulled): string {
  // Newest first, within the budget: bodies and file lists go before whole commits.
  const commits: string[] = [];
  let left = PULLED_BUDGET;
  let omitted = pulled.truncated;
  for (const [i, c] of pulled.commits.entries()) {
    const head = `- ${c.commit.summary} (${c.commit.author})`;
    const files = c.files.map((f) => `  ${f.path} (${f.added === null ? "binary" : `+${f.added} -${f.removed}`})`);
    if (c.more_files) files.push(`  and ${count(c.more_files, "more file")}`);
    const body = c.body ? `\n  ${c.body.replace(/\n/g, "\n  ")}` : "";
    const full = `${head}${body}\n${files.join("\n")}`;
    const entry = [full, `${head}\n${files.slice(0, 5).join("\n")}`, head].find((e) => bytes(e) + 1 <= left);
    if (entry === undefined) {
      omitted += pulled.commits.length - i;
      break;
    }
    commits.push(entry);
    left -= bytes(entry) + 1;
  }
  if (omitted) commits.push(`(${count(omitted, "older commit")} not listed)`);
  return [
    `Summarize what's new in ${count(pulled.commits.length + pulled.truncated, "commit")}`,
    "These commits came into the project with the last pull, merge or rebase. Tell the person what is new " +
      "for them in at most 8 short lines: features, fixes, breaking changes, new dependencies or steps they " +
      "must take (migrations, new settings). Plain sentences, one per line, no headings, no code blocks. " +
      "Do not run commands or edit files; you may read files in the project.",
    `Commits, newest first:\n${commits.join("\n")}`,
  ].join("\n\n");
}

const OPERATION_NAME: Record<GitOperation, string> = {
  merge: "a merge",
  rebase: "a rebase",
  cherry_pick: "a cherry-pick",
  revert: "a revert",
  bisect: "a bisect",
};

/** Asks what the conflicts are about and how they could be resolved. */
export function conflictsPrompt(files: readonly StatusFile[], operation: GitOperation | null): string {
  return [
    `Explain the conflicts in ${count(files.length, "file")}`,
    `The project is in the middle of ${operation ? OPERATION_NAME[operation] : "a merge"} with conflicts. ` +
      "Read the conflicted files (look for the <<<<<<<, ======= and >>>>>>> markers) and explain in at most " +
      "8 short lines what each side changed and how the person could resolve each conflict. Plain sentences, " +
      "no headings, no code blocks. Do not edit files or run commands: the person resolves them.",
    `Conflicted files:\n${files
      .slice(0, MAX_LISTED)
      .map((f) => `- ${f.path}`)
      .join("\n")}`,
  ].join("\n\n");
}

/** The text inside the last fenced block of `lang` (or any fence), if there is one. */
function fenced(text: string, lang: string): string | null {
  const blocks = [...text.matchAll(/```([a-z]*)[ \t]*\r?\n([\s\S]*?)```/gi)];
  const block = blocks.filter((m) => (m[1] ?? "").toLowerCase() === lang).at(-1) ?? blocks.at(-1);
  return block ? (block[2] ?? "") : null;
}

/** The commit message in an answer to `messagePrompt`; null when there is none. */
export function readMessage(answer: string): string | null {
  const text = (fenced(answer, "text") ?? answer).trim();
  return text && text.length <= MAX_MESSAGE ? text : null;
}

/**
 * The commits in an answer to `commitsPrompt`, checked against the changed
 * files: known paths only, none twice, messages non-empty and not too long.
 */
export function readPlan(
  answer: string,
  changed: readonly string[],
): { ok: true; commits: PlannedCommit[]; left: string[] } | { ok: false; why: string } {
  const block = fenced(answer, "json");
  if (block === null) return { ok: false, why: "it has no list of commits." };
  let data: unknown;
  try {
    data = JSON.parse(block);
  } catch {
    return { ok: false, why: "its list of commits is not valid JSON." };
  }
  const list = (data as { commits?: unknown } | null)?.commits;
  if (!Array.isArray(list) || list.length === 0) return { ok: false, why: "it suggests no commits." };
  if (list.length > MAX_COMMITS) return { ok: false, why: `it suggests more than ${MAX_COMMITS} commits.` };
  const known = new Set(changed);
  const used = new Set<string>();
  const commits: PlannedCommit[] = [];
  for (const item of list) {
    const { message, files } = (item ?? {}) as { message?: unknown; files?: unknown };
    if (typeof message !== "string" || !message.trim() || message.length > MAX_MESSAGE)
      return { ok: false, why: "a suggested commit has no usable message." };
    if (!Array.isArray(files) || files.length === 0 || !files.every((f) => typeof f === "string"))
      return { ok: false, why: "a suggested commit names no files." };
    for (const file of files as string[]) {
      if (!known.has(file)) return { ok: false, why: `it names ${file}, which has no changes.` };
      if (used.has(file)) return { ok: false, why: `it puts ${file} in two commits.` };
      used.add(file);
    }
    commits.push({ message: message.trim(), files: files as string[] });
  }
  return { ok: true, commits, left: changed.filter((f) => !used.has(f)) };
}

/** A short answer as plain lines for a card: markdown bullets and emphasis taken out. */
export function plainLines(answer: string, max = 12): string[] {
  return answer
    .split("\n")
    .map((line) =>
      line
        .replace(/^\s*(?:[-*•]|\d+[.)])\s+/, "")
        .replace(/^#+\s*/, "")
        .replace(/\*\*(.+?)\*\*/g, "$1")
        .replace(/`([^`]+)`/g, "$1")
        .trim(),
    )
    .filter((line) => line && !line.startsWith("```"))
    .slice(0, max);
}
