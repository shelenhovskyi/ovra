// "Explain these changes" in Reviews: the prompt for a read-only research
// session and the reading of its answer. Pure functions: `explain.svelte.ts`
// gathers the diffs and runs the session.

import { bytes, fitPrompt, hasToken, looksSecret, plainLines } from "../git/assist";
import type { FileChange, ReviewItem } from "../lib/protocol";

/** What the agent said: a few lines on the whole change, one line per file it explained. */
export type Explanation = { overall: string[]; files: Record<string, string> };

/** The most bytes of diff text one prompt carries, under the core's 64 KiB message limit. */
const DIFF_BUDGET = 40 * 1024;
/** The most bytes of one file's diff. */
const FILE_BUDGET = 12 * 1024;
/** The most files whose diff is asked for; the rest are named only. */
export const MAX_EXPLAINED = 60;
/** The most files listed by name. */
const MAX_LISTED = 300;
/** The most characters of one line the card shows. */
const MAX_LINE = 600;
/** The most characters of the agent's own summary the prompt carries. */
const MAX_SUMMARY = 2000;

/** Which files changed and by how much: an explanation made for other changes says so. */
export function explainedFor(item: ReviewItem): string {
  return JSON.stringify(item.files.map((f) => [f.path, f.kind, f.additions, f.deletions]));
}

function count(n: number, one: string): string {
  return `${n} ${one}${n === 1 ? "" : "s"}`;
}

function listed(files: readonly FileChange[]): string {
  const lines = files.slice(0, MAX_LISTED).map((f) => {
    const from = f.from ? `, renamed from ${f.from}` : "";
    return `- ${f.path} (${f.kind}${from}, +${f.additions} -${f.deletions})`;
  });
  if (files.length > MAX_LISTED) lines.push(`- and ${count(files.length - MAX_LISTED, "more file")}`);
  return lines.join("\n");
}

/** `patch` cut on a line to at most `budget` bytes. */
function cut(patch: string, budget: number): { text: string; cut: boolean } {
  if (bytes(patch) <= budget) return { text: patch, cut: false };
  const lines: string[] = [];
  let size = 0;
  for (const line of patch.split("\n")) {
    const length = bytes(line) + 1;
    if (size + length > budget) break;
    lines.push(line);
    size += length;
  }
  return { text: lines.join("\n"), cut: true };
}

/** The diffs within the budgets; files that may hold a secret are named without content. */
function diffsSection(patches: readonly { path: string; patch: string }[], files: readonly FileChange[]): string {
  const from = new Map(files.map((f) => [f.path, f.from]));
  const parts: string[] = [];
  const short: string[] = [];
  const hidden: string[] = [];
  let left = DIFF_BUDGET;
  for (const { path, patch } of patches) {
    const old = from.get(path);
    if (looksSecret(path) || (old && looksSecret(old)) || hasToken(patch)) {
      hidden.push(path);
      continue;
    }
    if (left < 256) {
      short.push(path);
      continue;
    }
    const { text, cut: wasCut } = cut(patch, Math.min(FILE_BUDGET, left));
    if (text.trim()) {
      parts.push(`--- ${path}\n${text}`);
      left -= bytes(text) + path.length + 5;
    }
    if (wasCut || !text.trim()) short.push(path);
  }
  const notes = [
    short.length ? `Cut or left out: ${short.join(", ")}` : "",
    hidden.length ? `Contents not sent because they may hold a secret: ${hidden.join(", ")}` : "",
  ].filter(Boolean);
  return [...parts, ...notes].join("\n\n") || "(no textual changes)";
}

/**
 * Asks for plain words on what changed and why: a few lines on the whole,
 * one line per file. The agent's diffs are other text than the person's,
 * so the session runs without web search (`no_web`).
 */
export function explainPrompt(
  item: ReviewItem,
  task: string,
  patches: readonly { path: string; patch: string }[],
): string {
  const summary = item.summary?.trim().slice(0, MAX_SUMMARY);
  return fitPrompt(
    [
      `Explain these changes in ${count(item.files.length, "file")}`,
      "An agent made the changes below for the task named here. The person reviewing them may not be a " +
        "programmer. Explain in plain, everyday words what changed and why, as you would to a colleague " +
        "from another team: no jargon where a plain word works, no code, no judging whether it is good. " +
        "Say what each change is for when the task or the code makes it clear; when it does not, say what " +
        "it does and do not guess. Do not edit files or run commands. You may read files in the project " +
        "for context; they may show the version before or after these changes.",
      "Answer with one short sentence, then exactly one ```json block, nothing after it:\n" +
        '```json\n{"overall": ["one to three short sentences on what the changes do together and why"], ' +
        '"files": [{"path": "path/one", "explanation": "one or two short sentences: what changed in this file and why"}]}\n```\n' +
        "Use only paths from the list of changed files, each at most once.",
      `Task: ${task}`,
      summary ? `The agent's own summary:\n${summary}` : "",
      `Changed files:\n${listed(item.files)}`,
      `Diffs:\n${diffsSection(patches, item.files)}`,
    ]
      .filter(Boolean)
      .join("\n\n"),
  );
}

function line(text: string): string {
  const plain = plainLines(text, 4).join(" ");
  return plain.length > MAX_LINE ? `${plain.slice(0, MAX_LINE - 1)}…` : plain;
}

/** The text inside the last ```json fence, if there is one. */
function jsonBlock(text: string): string | null {
  const blocks = [...text.matchAll(/```([a-z]*)[ \t]*\r?\n([\s\S]*?)```/gi)];
  const block = blocks.filter((m) => (m[1] ?? "").toLowerCase() === "json").at(-1) ?? blocks.at(-1);
  return block ? (block[2] ?? "") : null;
}

/**
 * The explanation in an answer to `explainPrompt`. Lines for paths that are
 * not among `paths` are dropped. An answer without a usable json block is
 * shown as its first plain lines; null when it says nothing.
 */
export function readExplanation(answer: string, paths: readonly string[]): Explanation | null {
  const known = new Set(paths);
  const block = jsonBlock(answer);
  let data: unknown = null;
  if (block !== null) {
    try {
      data = JSON.parse(block);
    } catch {
      data = null;
    }
  }
  if (data && typeof data === "object") {
    const { overall, files } = data as { overall?: unknown; files?: unknown };
    const lines = (Array.isArray(overall) ? overall : [overall])
      .filter((o): o is string => typeof o === "string")
      .map(line)
      .filter(Boolean)
      .slice(0, 4);
    const out: Record<string, string> = {};
    for (const entry of Array.isArray(files) ? files : []) {
      const { path, explanation } = (entry ?? {}) as { path?: unknown; explanation?: unknown };
      if (typeof path !== "string" || typeof explanation !== "string" || !known.has(path) || path in out) continue;
      const text = line(explanation);
      if (text) out[path] = text;
    }
    if (lines.length || Object.keys(out).length) return { overall: lines, files: out };
  }
  // Without the json, what is outside code blocks; a broken block is no explanation.
  const plain = plainLines(answer.replace(/```[\s\S]*?(```|$)/g, ""), 6).map((l) =>
    l.length > MAX_LINE ? `${l.slice(0, MAX_LINE - 1)}…` : l,
  );
  return plain.length ? { overall: plain, files: {} } : null;
}
