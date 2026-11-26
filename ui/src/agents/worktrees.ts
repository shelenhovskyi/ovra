// Words for a worker's worktree (docs/specs/worktree-setup.md): its setup
// steps, its size on disk and why it waits.

import type { DiskUsage, SetupProgress } from "../lib/protocol";

/** The sizes added up; null when none is known. A part counted only in part makes the sum "more than". */
export function diskTotal(parts: (DiskUsage | null | undefined)[]): DiskUsage | null {
  const known = parts.filter((d): d is DiskUsage => d != null);
  if (known.length === 0) return null;
  return { bytes: known.reduce((n, d) => n + d.bytes, 0), complete: known.every((d) => d.complete) };
}

/** `2 of 3: pnpm install`. */
export function stepLine(setup: SetupProgress): string {
  return `${setup.step + 1} of ${setup.steps}: ${setup.command}`;
}

/** A preparing worker's state line. */
export function preparingLabel(setup: SetupProgress | null | undefined): string {
  if (!setup) return "Preparing worktree";
  if (setup.failed !== null) return `Setup failed at ${stepLine(setup)}`;
  return `Preparing worktree · ${stepLine(setup)}`;
}

/** A queued worker's state line from the core's reason ("waits for disk space: ..."): its first part. */
export function waitingReason(reason: string): string {
  const first = reason.split(":")[0]?.trim() ?? reason;
  return first.startsWith("waits ") ? `Waiting ${first.slice("waits ".length)}` : `Waiting: ${first}`;
}

/** The last part of a path, for a list that shows the whole path on hover. */
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}
