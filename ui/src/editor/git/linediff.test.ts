import { describe, expect, test } from "vitest";
import { changeKind, diffLines, hunkAtLine, type LineHunk, mapLine, splitLines } from "./linediff";

/** Applies `hunks` to `a`, taking new lines from `b`: must give `b` back. */
function apply(a: string[], b: string[], hunks: LineHunk[]): string[] {
  const out: string[] = [];
  let i = 0;
  for (const h of hunks) {
    out.push(...a.slice(i, h.oldStart), ...b.slice(h.newStart, h.newStart + h.newCount));
    i = h.oldStart + h.oldCount;
  }
  return [...out, ...a.slice(i)];
}

describe("diffLines", () => {
  test("finds added, modified and deleted lines", () => {
    const a = splitLines("one\ntwo\nthree\nfour\nfive\n");
    const b = splitLines("zero\none\nTWO\nthree\nfive\n");
    const hunks = diffLines(a, b);
    expect(hunks).toEqual([
      { oldStart: 0, oldCount: 0, newStart: 0, newCount: 1 },
      { oldStart: 1, oldCount: 1, newStart: 2, newCount: 1 },
      { oldStart: 3, oldCount: 1, newStart: 4, newCount: 0 },
    ]);
    expect(hunks.map(changeKind)).toEqual(["added", "modified", "deleted"]);
    expect(diffLines(a, a)).toEqual([]);
    expect(splitLines("a\r\nb\rc")).toEqual(["a", "b", "c"]);
  });

  test("its hunks rebuild the new text for random edits", () => {
    let seed = 7;
    const random = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed % n;
    };
    for (let round = 0; round < 200; round++) {
      const a = Array.from({ length: random(12) }, () => `l${random(5)}`);
      const b = Array.from({ length: random(12) }, () => `l${random(5)}`);
      const hunks = diffLines(a, b);
      expect(apply(a, b, hunks)).toEqual(b);
      // No empty hunks.
      for (const h of hunks) expect(h.oldCount + h.newCount).toBeGreaterThan(0);
    }
  });

  test("gives up on the middle of very different texts instead of searching on", () => {
    const a = Array.from({ length: 3000 }, (_, i) => `a${i}`);
    const b = ["same", ...Array.from({ length: 3000 }, (_, i) => `b${i}`)];
    expect(diffLines(["same", ...a], b)).toEqual([{ oldStart: 1, oldCount: 3000, newStart: 1, newCount: 3000 }]);
  });
});

describe("line lookups", () => {
  const hunks = diffLines(splitLines("a\nb\nc\nd"), splitLines("a\nB\nc\nx\ny"));
  // b -> B (line 1), d -> x, y (lines 3-4).
  test("hunkAtLine finds the hunk under a marker, a deletion on the line above", () => {
    expect(hunkAtLine(hunks, 1)?.newStart).toBe(1);
    expect(hunkAtLine(hunks, 2)).toBeUndefined();
    expect(hunkAtLine(hunks, 4)?.newStart).toBe(3);
    const deleted = diffLines(["a", "b", "c"], ["a", "c"]);
    expect(hunkAtLine(deleted, 0)).toEqual(deleted[0]);
  });

  test("mapLine follows unchanged lines and drops changed ones", () => {
    expect([0, 1, 2, 3, 4].map((line) => mapLine(hunks, line))).toEqual([0, null, 2, null, null]);
    expect(mapLine(diffLines(["a", "b"], ["new", "a", "b"]), 2)).toBe(1);
  });
});
