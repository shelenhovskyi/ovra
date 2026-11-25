// Replace in files: the core reports matches (search.*) and the UI applies
// the replacement. The core matches with Rust's regex engine; here a
// JavaScript RegExp built from the same query finds each line's matches
// again, and a line is replaced only when they are the ones the core
// reported, so the two engines can never disagree silently.

import type { FileMatches, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export type ReplaceQuery = {
  pattern: string;
  regex: boolean;
  caseSensitive: boolean;
  wholeWord: boolean;
  /** With `regex`, `$1`, `$<name>`, `$&` and `$$` work as in JavaScript. */
  replacement: string;
};

/** A change in a text; offsets are UTF-16 code units, like the core's ranges. */
export type Edit = { from: number; to: number; insert: string };

/** Rust's `\w` (Unicode): what whole-word matching treats as a word character. */
const WORD = "[\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}\\p{Join_Control}]";

/**
 * The query as a global RegExp, or null when JavaScript cannot read the
 * pattern (Rust-only syntax such as `(?i)` or `[[:alpha:]]`).
 */
export function matcher(query: Omit<ReplaceQuery, "replacement">): RegExp | null {
  let source = query.regex ? query.pattern : query.pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (query.wholeWord) source = `(?<!${WORD})(?:${source})(?!${WORD})`;
  const flags = query.caseSensitive ? "gu" : "giu";
  try {
    return new RegExp(source, flags);
  } catch {
    return null;
  }
}

/** The text that replaces `match` in `line`, with JavaScript's `$` patterns. */
export function expand(template: string, match: RegExpExecArray, line: string): string {
  const groups = match.length - 1;
  return template.replace(/\$(\$|&|`|'|<([^>]*)>|(\d\d?))/g, (whole, what: string, name?: string, digits?: string) => {
    if (what === "$") return "$";
    if (what === "&") return match[0];
    if (what === "`") return line.slice(0, match.index);
    if (what === "'") return line.slice(match.index + match[0].length);
    if (name !== undefined) return match.groups ? (match.groups[name] ?? "") : whole;
    if (digits === undefined) return whole;
    const two = Number(digits);
    if (digits.length === 2 && two >= 1 && two <= groups) return match[two] ?? "";
    const one = Number(digits[0]);
    if (one >= 1 && one <= groups) return (match[one] ?? "") + digits.slice(1);
    return whole;
  });
}

/** The core cuts lines longer than this many UTF-8 bytes to a window around the first match. */
const CUT_BYTES = 400;
const encoder = new TextEncoder();

export type FilePlan = {
  edits: Edit[];
  /** Matches that will be replaced. */
  replaced: number;
  /** Some line is not what the search found: the file changed since. */
  changed: boolean;
  /** Reported matches the RegExp did not find the same way; left as they are. */
  skipped: number;
};

/**
 * The edits that replace `file`'s matches in `text` (all of them, or only
 * those on `only` lines). Line breaks are `\n` or `\r\n`, as the core reads them.
 */
export function planFile(text: string, file: FileMatches, query: ReplaceQuery, only?: number): FilePlan {
  const plan: FilePlan = { edits: [], replaced: 0, changed: false, skipped: 0 };
  const re = matcher(query);
  if (!re) return { ...plan, skipped: file.lines.reduce((n, l) => n + l.ranges.length, 0) };
  const starts = [0];
  for (let at = text.indexOf("\n"); at >= 0; at = text.indexOf("\n", at + 1)) starts.push(at + 1);
  for (const found of file.lines) {
    if (only !== undefined && found.line !== only) continue;
    const start = starts[found.line - 1];
    if (start === undefined) return { ...plan, changed: true };
    let end = starts[found.line] === undefined ? text.length : (starts[found.line] as number) - 1;
    if (text[end - 1] === "\r") end--;
    const line = text.slice(start, end);
    const windowEnd = found.column + found.text.length;
    const cut = found.column > 0 || found.text.length < line.length;
    if (line.slice(found.column, windowEnd) !== found.text || (cut && encoder.encode(line).length <= CUT_BYTES)) {
      return { ...plan, changed: true };
    }
    const matches: RegExpExecArray[] = [];
    re.lastIndex = 0;
    for (let m = re.exec(line); m; m = re.exec(line)) {
      if (m[0] === "") re.lastIndex++;
      else matches.push(m);
    }
    // What the core would report for these matches: clamped to its window.
    const seen = matches.flatMap((m) => {
      const [from, to] = [m.index, m.index + m[0].length];
      if (from >= windowEnd || to <= found.column) return [];
      return [`${Math.max(from, found.column) - found.column},${Math.min(to, windowEnd) - found.column}`];
    });
    if (seen.join(" ") !== found.ranges.map(([from, to]) => `${from},${to}`).join(" ")) {
      plan.skipped += found.ranges.length;
      continue;
    }
    for (const m of matches) {
      const insert = query.regex ? expand(query.replacement, m, line) : query.replacement;
      plan.edits.push({ from: start + m.index, to: start + m.index + m[0].length, insert });
    }
    plan.replaced += matches.length;
  }
  return plan;
}

/** `text` with `edits` (in order, not overlapping) applied. */
export function applyEdits(text: string, edits: readonly Edit[]): string {
  let out = "";
  let at = 0;
  for (const edit of edits) {
    out += text.slice(at, edit.from) + edit.insert;
    at = edit.to;
  }
  return out + text.slice(at);
}

/** An open editor buffer: edits to it stay unsaved and can be undone. */
export type OpenBuffer = {
  readonly loaded: Promise<void>;
  readonly status: string;
  readonly text: string | null;
  edit(spec: { changes: Edit[] }): boolean;
};

export type ReplaceTarget = {
  transport: Transport;
  project: ProjectId;
  /** The open buffer of `path`, if any. */
  buffer: (path: string) => OpenBuffer | undefined;
};

export type ReplaceOutcome = {
  replaced: number;
  /** Paths where something was replaced. */
  files: string[];
  /** Paths left alone because they changed since the search. */
  changed: string[];
  /** Paths whose write was refused because they changed while replacing. */
  conflicts: string[];
  skipped: number;
  /** Other failures, as messages. */
  errors: string[];
};

/**
 * Replaces the matches of `files` (only line `only` with one file): in the
 * open buffer if the file has one, otherwise on disk, never over a change
 * made since the file was read.
 */
export async function replaceInFiles(
  target: ReplaceTarget,
  files: readonly FileMatches[],
  query: ReplaceQuery,
  only?: number,
): Promise<ReplaceOutcome> {
  const out: ReplaceOutcome = { replaced: 0, files: [], changed: [], conflicts: [], skipped: 0, errors: [] };
  for (const file of files) {
    try {
      await replaceInFile(target, file, query, only, out);
    } catch (err) {
      if (err instanceof CoreError && err.code === "conflict") out.conflicts.push(file.path);
      else out.errors.push(`${file.path}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }
  return out;
}

async function replaceInFile(
  target: ReplaceTarget,
  file: FileMatches,
  query: ReplaceQuery,
  only: number | undefined,
  out: ReplaceOutcome,
): Promise<void> {
  const buffer = target.buffer(file.path);
  await buffer?.loaded;
  const open = buffer?.status === "ready" ? buffer.text : null;
  let text = open;
  let mtime = 0;
  if (text === null) {
    const { data } = await target.transport.call({
      method: "fs.read_file",
      params: { project: target.project, path: file.path },
    });
    if (data.body.kind !== "text") {
      out.changed.push(file.path);
      return;
    }
    text = data.body.text;
    mtime = data.mtime_ms;
  }
  const plan = planFile(text, file, query, only);
  out.skipped += plan.skipped;
  if (plan.changed) {
    out.changed.push(file.path);
    return;
  }
  if (plan.edits.length === 0) return;
  if (open !== null && buffer) {
    if (!buffer.edit({ changes: plan.edits })) {
      out.changed.push(file.path);
      return;
    }
  } else {
    await target.transport.call({
      method: "fs.write_file",
      params: {
        project: target.project,
        path: file.path,
        text: applyEdits(text, plan.edits),
        expected_mtime_ms: mtime,
      },
    });
  }
  out.replaced += plan.replaced;
  out.files.push(file.path);
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** What a replace did, in a few words. */
export function describe(out: ReplaceOutcome): string {
  const parts = [
    out.replaced > 0
      ? `Replaced ${count(out.replaced, "match", "matches")} in ${count(out.files.length, "file", "files")}.`
      : "Nothing replaced.",
  ];
  if (out.changed.length) parts.push(`${count(out.changed.length, "file", "files")} changed since the search.`);
  if (out.conflicts.length) parts.push(`${count(out.conflicts.length, "file", "files")} changed while replacing.`);
  if (out.skipped) parts.push(`${count(out.skipped, "match", "matches")} could not be replaced.`);
  if (out.errors.length) parts.push(...out.errors);
  return parts.join(" ");
}
