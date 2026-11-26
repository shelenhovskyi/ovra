// The hunks of one file's review diff, each as the protocol `Hunk` that
// `review.discard_hunk` sends back and as numbered lines to show.

import type { Hunk } from "../lib/protocol";

export type HunkLine =
  | { kind: "context"; text: string; old: number; new: number }
  | { kind: "added"; text: string; new: number }
  | { kind: "removed"; text: string; old: number }
  | { kind: "note"; text: string };

export type ParsedHunk = {
  hunk: Hunk;
  lines: HunkLine[];
  /** Names the hunk by its text, so a mark on it survives lines moving above it. */
  key: string;
};

const HEAD = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/;

/**
 * The hunks of `patch`, a unified diff of one file. Lines keep a CRLF
 * file's `\r` in `hunk.lines`, as the core needs them to match the file.
 */
export function parseHunks(patch: string): ParsedHunk[] {
  const raw = patch.split("\n");
  if (raw.at(-1) === "") raw.pop();
  const hunks: ParsedHunk[] = [];
  let i = 0;
  while (i < raw.length) {
    const head = HEAD.exec((raw[i] ?? "").replace(/\r$/, ""));
    i++;
    if (!head) continue;
    const count = (n: string | undefined) => (n === undefined ? 1 : Number(n));
    const hunk: Hunk = {
      old_start: Number(head[1]),
      old_lines: count(head[2]),
      new_start: Number(head[3]),
      new_lines: count(head[4]),
      header: head[5] ?? "",
      lines: [],
    };
    const lines: HunkLine[] = [];
    let [oldLeft, newLeft] = [hunk.old_lines, hunk.new_lines];
    let [oldNo, newNo] = [hunk.old_start, hunk.new_start];
    while (i < raw.length) {
      const line = raw[i] ?? "";
      const text = line.slice(1).replace(/\r$/, "");
      if (line.startsWith("\\") && hunk.lines.length > 0) {
        hunk.lines.push(line.replace(/\r$/, ""));
        lines.push({ kind: "note", text: line });
      } else if (oldLeft <= 0 && newLeft <= 0) break;
      else if (line.startsWith("+")) {
        lines.push({ kind: "added", text, new: newNo++ });
        newLeft--;
        hunk.lines.push(line);
      } else if (line.startsWith("-")) {
        lines.push({ kind: "removed", text, old: oldNo++ });
        oldLeft--;
        hunk.lines.push(line);
      } else if (line.startsWith(" ")) {
        lines.push({ kind: "context", text, old: oldNo++, new: newNo++ });
        oldLeft--;
        newLeft--;
        hunk.lines.push(line);
      } else break;
      i++;
    }
    if (hunk.lines.length > 0) hunks.push({ hunk, lines, key: hunk.lines.join("\n") });
  }
  return hunks;
}

/** "lines 12–18" of the agent's version, or of the old one when it removed them all. */
export function hunkPlace({ hunk }: ParsedHunk): string {
  const [start, length] = hunk.new_lines > 0 ? [hunk.new_start, hunk.new_lines] : [hunk.old_start, hunk.old_lines];
  const range = length <= 1 ? `line ${start}` : `lines ${start}–${start + length - 1}`;
  return hunk.new_lines > 0 ? range : `${range} of the old version`;
}

/** The line a comment on `line` is about: its number in the agent's version, else in the old one. */
export function commentLine(line: HunkLine): { line: number; old: boolean } | null {
  if (line.kind === "note") return null;
  if (line.kind === "removed") return { line: line.old, old: true };
  return { line: line.new, old: false };
}
