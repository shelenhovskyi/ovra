// Fuzzy matching for the palette: the query's characters must appear in
// order; matches at word starts (after a space, `/`, `.`, `-`, `_` or a case
// change) and in runs score higher. O(query x text) per candidate, so
// ranking tens of thousands of paths per keystroke stays fast.

export type Match = { score: number; positions: number[] };

const WORD_START = 8;
const RUN = 6;
const GAP = 0.2;
const NONE = Number.NEGATIVE_INFINITY;

function isWordStart(text: string, i: number): boolean {
  if (i === 0) return true;
  const prev = text[i - 1] ?? "";
  const cur = text[i] ?? "";
  if (/[\s._\-/:]/.test(prev)) return true;
  return prev === prev.toLowerCase() && cur !== cur.toLowerCase();
}

function isSubsequence(q: string, t: string): boolean {
  let i = 0;
  for (let j = 0; j < t.length && i < q.length; j++) if (t[j] === q[i]) i++;
  return i === q.length;
}

/** Best alignment of `query` in `text`, or null when it does not match. */
export function fuzzyMatch(query: string, text: string): Match | null {
  const q = query.toLowerCase().replaceAll(" ", "");
  const t = text.toLowerCase();
  if (q.length === 0) return { score: 0, positions: [] };
  if (q.length > t.length || !isSubsequence(q, t)) return null;

  const m = t.length;
  // best[i*m+j]: best score with q[i] matched at t[j]; from[i*m+j]: where q[i-1] matched.
  const best = new Float64Array(q.length * m).fill(NONE);
  const from = new Int32Array(q.length * m).fill(-1);
  for (let j = 0; j < m; j++) {
    if (t[j] === q[0]) best[j] = 1 + (isWordStart(text, j) ? WORD_START : 0) - j * GAP;
  }
  for (let i = 1; i < q.length; i++) {
    const row = i * m;
    const prev = row - m;
    // Best previous match at k <= j - 2, already charged for the gap up to j.
    let gapScore = NONE;
    let gapFrom = -1;
    for (let j = i; j < m; j++) {
      if (j >= 2) {
        const candidate = (best[prev + j - 2] as number) - GAP;
        gapScore -= GAP;
        if (candidate > gapScore) {
          gapScore = candidate;
          gapFrom = j - 2;
        }
      }
      if (t[j] !== q[i]) continue;
      const own = 1 + (isWordStart(text, j) ? WORD_START : 0);
      const run = (best[prev + j - 1] as number) + RUN;
      if (run >= gapScore && run > NONE) {
        best[row + j] = run + own;
        from[row + j] = j - 1;
      } else if (gapScore > NONE) {
        best[row + j] = gapScore + own;
        from[row + j] = gapFrom;
      }
    }
  }

  const last = (q.length - 1) * m;
  let end = -1;
  for (let j = 0; j < m; j++) if (end < 0 || (best[last + j] as number) > (best[last + end] as number)) end = j;
  if (end < 0 || best[last + end] === NONE) return null;

  const positions: number[] = new Array(q.length);
  for (let i = q.length - 1, j = end; i >= 0; i--) {
    positions[i] = j;
    j = from[i * m + j] as number;
  }
  return { score: best[last + end] as number, positions };
}

export type Ranked<T> = { item: T; positions: number[] };

/** Items whose text matches `query`, best first, at most `limit`; all of them, in order, for an empty query. */
export function rank<T>(items: readonly T[], text: (item: T) => string, query: string, limit = Infinity): Ranked<T>[] {
  if (query.trim() === "") return items.slice(0, limit).map((item) => ({ item, positions: [] }));
  const matched: (Ranked<T> & { score: number; text: string })[] = [];
  for (const item of items) {
    const label = text(item);
    const match = fuzzyMatch(query, label);
    if (match) matched.push({ item, text: label, ...match });
  }
  return matched
    .sort((a, b) => b.score - a.score || a.text.length - b.text.length || a.text.localeCompare(b.text))
    .slice(0, limit)
    .map(({ item, positions }) => ({ item, positions }));
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
