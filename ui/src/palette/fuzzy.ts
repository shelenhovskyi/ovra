// Fuzzy matching for the command palette: the query's characters must appear
// in order; matches at word starts and in runs score higher.

import type { Command } from "../keys/commands";

export type Match = { score: number; positions: number[] };

const WORD_START = 8;
const RUN = 6;
const GAP = 0.2;

function isWordStart(text: string, i: number): boolean {
  if (i === 0) return true;
  const prev = text[i - 1] ?? "";
  const cur = text[i] ?? "";
  if (/[\s._\-/:]/.test(prev)) return true;
  return prev === prev.toLowerCase() && cur !== cur.toLowerCase();
}

/** Best alignment of `query` in `text`, or null when it does not match. */
export function fuzzyMatch(query: string, text: string): Match | null {
  const q = query.toLowerCase().replaceAll(" ", "");
  const t = text.toLowerCase();
  if (q.length === 0) return { score: 0, positions: [] };
  if (q.length > t.length) return null;

  // best[i][j]: best score with q[i] matched at t[j]; from[i][j]: where q[i-1] matched.
  const best = q.split("").map(() => new Float64Array(t.length).fill(Number.NEGATIVE_INFINITY));
  const from = q.split("").map(() => new Int32Array(t.length).fill(-1));
  for (let i = 0; i < q.length; i++) {
    const row = best[i] as Float64Array;
    const prevRow = best[i - 1];
    for (let j = i; j < t.length; j++) {
      if (t[j] !== q[i]) continue;
      const own = 1 + (isWordStart(text, j) ? WORD_START : 0);
      if (!prevRow) {
        row[j] = own - j * GAP;
        continue;
      }
      for (let k = i - 1; k < j; k++) {
        const prev = prevRow[k] as number;
        if (prev === Number.NEGATIVE_INFINITY) continue;
        const score = prev + own + (k === j - 1 ? RUN : -(j - k - 1) * GAP);
        if (score > (row[j] as number)) {
          row[j] = score;
          (from[i] as Int32Array)[j] = k;
        }
      }
    }
  }

  const last = best[q.length - 1] as Float64Array;
  let end = -1;
  for (let j = 0; j < t.length; j++) if (end < 0 || (last[j] as number) > (last[end] as number)) end = j;
  if (end < 0 || last[end] === Number.NEGATIVE_INFINITY) return null;

  const positions: number[] = [];
  for (let i = q.length - 1, j = end; i >= 0; j = (from[i] as Int32Array)[j] as number, i--) positions.unshift(j);
  return { score: last[end] as number, positions };
}

export type Item = { command: Command; positions: number[] };

/** Commands whose title matches `query`, best first; all of them, in order, for an empty query. */
export function filterCommands(commands: Command[], query: string): Item[] {
  if (query.trim() === "") return commands.map((command) => ({ command, positions: [] }));
  const items: (Item & { score: number })[] = [];
  for (const command of commands) {
    const match = fuzzyMatch(query, command.title);
    if (match) items.push({ command, ...match });
  }
  return items
    .sort((a, b) => b.score - a.score || a.command.title.localeCompare(b.command.title))
    .map(({ command, positions }) => ({ command, positions }));
}

/** Splits `text` into runs of matched and unmatched characters for highlighting. */
export function segments(text: string, positions: number[]): { text: string; hit: boolean }[] {
  const hits = new Set(positions);
  const out: { text: string; hit: boolean }[] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = hits.has(i);
    const last = out.at(-1);
    if (last && last.hit === hit) last.text += text[i];
    else out.push({ text: text[i] ?? "", hit });
  }
  return out;
}
