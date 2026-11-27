// "Go to file" with a line, as compilers and stack traces print places:
// `main.rs:42`, `main.rs:42:7`, `main.rs(42)` or `main.rs(42,7)`. The part
// before the suffix ranks the files; the file picked opens in the editor there.

import type { TextSpot } from "../shell/workspace.svelte";

/** `name:line[:column]`, or `name(line[,column])`; a suffix still being typed (`name:`) counts. */
const SUFFIX = /^(.*?)(?::(\d*)(?::(\d*))?|\((\d+)(?:[,:] ?(\d*))?\)?)$/;

/** The file part of `query`, and the spot its suffix names (null without a line number). */
export function splitLine(query: string): { text: string; spot: TextSpot | null } {
  // A trailing colon after the numbers, as `grep -n` and compilers print them.
  const match = SUFFIX.exec(query.trimEnd().replace(/(\d):$/, "$1"));
  if (!match) return { text: query, spot: null };
  const text = match[1] ?? "";
  const line = match[2] || match[4];
  if (!line) return { text, spot: null };
  // Columns are typed from 1; spots count UTF-16 offsets from 0. The editor clamps both.
  const from = Math.max(Number(match[3] || match[5] || 1) - 1, 0);
  return { text, spot: { line: Number(line), from, to: from } };
}

/** "Line 42", or "Line 42, column 7", for the rows that open there. */
export function spotLabel(spot: TextSpot): string {
  return spot.from > 0 ? `Line ${spot.line}, column ${spot.from + 1}` : `Line ${spot.line}`;
}
