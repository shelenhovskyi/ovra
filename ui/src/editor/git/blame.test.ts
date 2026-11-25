import { describe, expect, test } from "vitest";
import type { Blame } from "../../lib/protocol";
import { blameBlocks, blockLabel, commitsByLine, lineBlame, relativeTime } from "./blame";

const NOW = Date.UTC(2026, 10, 25);
const DAY = 86400;

const blame: Blame = {
  ranges: [
    { start: 1, lines: 2, commit: "c1" },
    { start: 3, lines: 1, commit: null },
    { start: 4, lines: 2, commit: "c2" },
  ],
  commits: {
    c1: { id: "c1", author: "Ann", time: NOW / 1000 - 3 * DAY, summary: "Start" },
    c2: { id: "c2", author: "Bo", time: NOW / 1000 - 400 * DAY, summary: "Fix" },
  },
};

describe("blame", () => {
  test("groups lines from the same commit into blocks", () => {
    const blocks = blameBlocks(blame, 5, (line) => line);
    expect(blocks.map((b) => [b.line, blockLabel(b.commit, NOW)])).toEqual([
      [0, "Ann, 3 days ago"],
      [2, "Not committed yet"],
      [3, "Bo, 1 year ago"],
    ]);
  });

  test("maps buffer lines to disk; edited lines are not committed yet", () => {
    // Buffer line 1 is edited, a line was inserted before it, the rest moved down one.
    const toDisk = (line: number) => (line === 1 || line === 2 ? null : line === 0 ? 0 : line - 1);
    expect(blameBlocks(blame, 6, toDisk).map((b) => b.line)).toEqual([0, 1, 4]);
    const commits = commitsByLine(blame);
    expect(lineBlame(blame, commits, 0, NOW)).toBe("Ann, 3 days ago · Start");
    expect(lineBlame(blame, commits, null, NOW)).toBe("Not committed yet");
  });

  test("says how long ago, in the largest whole unit", () => {
    const at = (seconds: number) => relativeTime(NOW / 1000 - seconds, NOW);
    expect([at(5), at(60), at(7200), at(8 * DAY), at(45 * DAY), at(-10)]).toEqual([
      "just now",
      "1 minute ago",
      "2 hours ago",
      "1 week ago",
      "1 month ago",
      "just now",
    ]);
  });
});
