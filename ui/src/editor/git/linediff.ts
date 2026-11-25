// A line diff (Myers) for the gutter change markers and for mapping blame
// lines through unsaved edits. Lines and hunk positions are 0-based; a text
// ending in a line break has an empty last line, as in CodeMirror.

/** Old lines `oldStart..oldStart+oldCount` became new lines `newStart..newStart+newCount`. */
export type LineHunk = { oldStart: number; oldCount: number; newStart: number; newCount: number };

export type ChangeKind = "added" | "modified" | "deleted";

/** Texts with more lines than this get no markers (the diff would cost too much per edit). */
export const MAX_DIFF_LINES = 20_000;

/**
 * Past this many inserted plus deleted lines the diff stops searching and
 * reports the differing middle as one hunk, so time and memory stay bounded.
 */
const MAX_EDIT_COST = 1_000;

/** Lines of `text`, split as CodeMirror splits them. */
export function splitLines(text: string): string[] {
  return text.split(/\r\n?|\n/);
}

/** The hunks that turn `a` into `b`, in order. */
export function diffLines(a: readonly string[], b: readonly string[]): LineHunk[] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  if (start === endA && start === endB) return [];
  // Comparing small integers is cheaper than comparing strings in the inner loop.
  const ids = new Map<string, number>();
  const intern = (line: string): number => {
    let id = ids.get(line);
    if (id === undefined) {
      id = ids.size;
      ids.set(line, id);
    }
    return id;
  };
  const x = Int32Array.from(a.slice(start, endA), intern);
  const y = Int32Array.from(b.slice(start, endB), intern);
  return myers(x, y).map((h) => ({ ...h, oldStart: h.oldStart + start, newStart: h.newStart + start }));
}

const EQUAL = 0;
const DELETE = 1;
const INSERT = 2;

function myers(a: Int32Array, b: Int32Array): LineHunk[] {
  const n = a.length;
  const m = b.length;
  const limit = Math.min(n + m, MAX_EDIT_COST);
  const offset = limit + 1;
  const v = new Int32Array(2 * limit + 3);
  // trace[d] holds v for k in -d-1..d+1 as it was before round d.
  const trace: Int32Array[] = [];
  for (let d = 0; d <= limit; d++) {
    trace.push(v.slice(offset - d - 1, offset + d + 2));
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && (v[offset + k - 1] as number) < (v[offset + k + 1] as number))
          ? (v[offset + k + 1] as number)
          : (v[offset + k - 1] as number) + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) return hunks(backtrack(trace, n, m));
    }
  }
  return [{ oldStart: 0, oldCount: n, newStart: 0, newCount: m }];
}

/** The edit script, first to last, from the snapshots of the forward search. */
function backtrack(trace: Int32Array[], n: number, m: number): number[] {
  const ops: number[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d] as Int32Array;
    const at = (k: number) => v[k + d + 1] as number;
    const k = x - y;
    const prevK = k === -d || (k !== d && at(k - 1) < at(k + 1)) ? k + 1 : k - 1;
    const prevX = at(prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      ops.push(EQUAL);
      x--;
      y--;
    }
    if (d > 0) ops.push(x === prevX ? INSERT : DELETE);
    x = prevX;
    y = prevY;
  }
  return ops.reverse();
}

function hunks(ops: number[]): LineHunk[] {
  const out: LineHunk[] = [];
  let i = 0;
  let j = 0;
  let current: LineHunk | null = null;
  for (const op of ops) {
    if (op === EQUAL) {
      current = null;
      i++;
      j++;
      continue;
    }
    if (!current) {
      current = { oldStart: i, oldCount: 0, newStart: j, newCount: 0 };
      out.push(current);
    }
    if (op === DELETE) {
      current.oldCount++;
      i++;
    } else {
      current.newCount++;
      j++;
    }
  }
  return out;
}

export function changeKind(hunk: LineHunk): ChangeKind {
  if (hunk.oldCount === 0) return "added";
  return hunk.newCount === 0 ? "deleted" : "modified";
}

/** The new line a hunk's marker sits on: its first line, or for a deletion the line above. */
export function markerLine(hunk: LineHunk): number {
  return hunk.newCount > 0 ? hunk.newStart : Math.max(hunk.newStart - 1, 0);
}

/** The hunk whose marker covers new line `line`, if any. */
export function hunkAtLine(hunks: readonly LineHunk[], line: number): LineHunk | undefined {
  return hunks.find((h) =>
    h.newCount > 0 ? line >= h.newStart && line < h.newStart + h.newCount : line === markerLine(h),
  );
}

/** The old line that new line `line` came from unchanged, or null for an added or changed line. */
export function mapLine(hunks: readonly LineHunk[], line: number): number | null {
  let delta = 0;
  for (const h of hunks) {
    if (line < h.newStart) break;
    if (line < h.newStart + h.newCount) return null;
    delta = h.oldStart + h.oldCount - (h.newStart + h.newCount);
  }
  return line + delta;
}
