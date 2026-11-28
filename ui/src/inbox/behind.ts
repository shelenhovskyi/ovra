// Words for a worker whose project moved on since it started
// (`docs/specs/main-moved.md`). Pure, so the wording is tested alone.

import type { Behind } from "../lib/protocol";
import type { Updated } from "./reviews.svelte";

const SHOWN = 3;

function files(n: number): string {
  return n === 1 ? "1 file" : `${n} files`;
}

/** "a.ts, b.ts and 2 more", by file name. */
export function named(paths: string[]): string {
  const names = paths.slice(0, SHOWN).map((p) => p.split("/").pop() ?? p);
  const rest = paths.length - names.length;
  if (rest > 0) return `${names.join(", ")} and ${rest} more`;
  if (names.length < 2) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** The warning above Keep. */
export function behindText(behind: Behind): string {
  if (behind.clashes.length === 0) {
    return `Your project changed in ${files(behind.files)} since the agent started, none that it changed.`;
  }
  const them = behind.clashes.length === 1 ? "it" : "them";
  return `Your project changed since this agent's copy was made, also in ${named(behind.clashes)}, which the agent changed too. Keep would leave ${them} out. Bring in your changes first to merge both.`;
}

/** What the last update did, for the notice and screen readers. */
export function updatedText(updated: Updated): string {
  if (!updated.written) {
    return updated.conflicts.length === 0
      ? "Nothing to bring in: the agent's copy already has your project's changes."
      : `${named(updated.conflicts)} changed in the same place on both sides, so nothing was brought in yet. Bringing changes in anyway marks those places for the agent to resolve.`;
  }
  const came = `Brought in ${files(updated.files)} from your project.`;
  return updated.conflicts.length === 0
    ? `${came} The diff now shows only the agent's changes.`
    : `${came} ${named(updated.conflicts)} now ${updated.conflicts.length === 1 ? "has" : "have"} conflict markers to resolve.`;
}

/** The comment that asks the agent to resolve the markers, for the person to send. */
export function resolveComment(conflicts: string[]): string {
  return `Resolve the conflict markers in ${conflicts.join(", ")}: keep both my changes and yours where you can, then ask for review again.`;
}
