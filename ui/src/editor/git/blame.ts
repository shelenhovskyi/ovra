// Turning `git.blame` into per-line labels for the blame gutter and the
// inline blame of the cursor line. Blame is of the file on disk; with unsaved
// edits, `toDisk` maps a buffer line to its line on disk, or null for an
// edited line, which counts as not committed yet.

import type { Blame, CommitInfo } from "../../lib/protocol";

const NOT_COMMITTED = "Not committed yet";

/** The commit id of every line on disk (0-based), null for uncommitted lines. */
export function commitsByLine(blame: Blame): (string | null)[] {
  const lines: (string | null)[] = [];
  for (const range of blame.ranges) {
    for (let i = 0; i < range.lines; i++) lines[range.start - 1 + i] = range.commit;
  }
  return lines;
}

/** "3 days ago" for a time in seconds since the epoch. */
export function relativeTime(seconds: number, nowMs: number): string {
  const elapsed = Math.max(0, nowMs / 1000 - seconds);
  const units: [string, number][] = [
    ["year", 365 * 86400],
    ["month", 30 * 86400],
    ["week", 7 * 86400],
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [name, size] of units) {
    const count = Math.floor(elapsed / size);
    if (count >= 1) return `${count} ${name}${count === 1 ? "" : "s"} ago`;
  }
  return "just now";
}

export type BlameBlock = { line: number; commit: CommitInfo | null };

/**
 * The first buffer line (0-based) of each run of lines from the same commit,
 * for `lineCount` buffer lines.
 */
export function blameBlocks(blame: Blame, lineCount: number, toDisk: (line: number) => number | null): BlameBlock[] {
  const commits = commitsByLine(blame);
  const blocks: BlameBlock[] = [];
  let previous: string | null | undefined;
  for (let line = 0; line < lineCount; line++) {
    const disk = toDisk(line);
    const id = disk === null ? null : (commits[disk] ?? null);
    if (line > 0 && id === previous) continue;
    previous = id;
    blocks.push({ line, commit: id === null ? null : (blame.commits[id] ?? null) });
  }
  return blocks;
}

/** The gutter label of a block. */
export function blockLabel(commit: CommitInfo | null, nowMs: number): string {
  return commit ? `${commit.author}, ${relativeTime(commit.time, nowMs)}` : NOT_COMMITTED;
}

/** The inline blame of one line on disk (null: edited in the buffer); `commits` from `commitsByLine`. */
export function lineBlame(blame: Blame, commits: (string | null)[], diskLine: number | null, nowMs: number): string {
  const id = diskLine === null ? null : (commits[diskLine] ?? null);
  const commit = id === null ? undefined : blame.commits[id];
  return commit ? `${blockLabel(commit, nowMs)} · ${commit.summary}` : NOT_COMMITTED;
}
