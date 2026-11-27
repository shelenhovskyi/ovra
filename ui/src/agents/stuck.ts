// Words for a working agent that may be stuck (`docs/specs/stuck-and-resume.md`).
// Only a hint: the person decides whether to stop it or send it a nudge.

import type { Stuck } from "../lib/protocol";

const MAX_COMMAND = 60;

function short(text: string): string {
  const line = text.split("\n")[0] ?? "";
  return line.length > MAX_COMMAND ? `${line.slice(0, MAX_COMMAND - 1)}…` : line;
}

/** What the repeated call did, as the person knows it: its command, its file or its tool. */
function what(stuck: Extract<Stuck, { kind: "repeating" }>): string {
  const { call, name } = stuck;
  if (call.command) return `ran ${short(call.command)}`;
  const path = call.paths?.[0];
  if (path) return `${call.kind === "read" ? "read" : "used"} ${path.split(/[\\/]/).pop() ?? path}`;
  if (call.url) return `fetched ${short(call.url)}`;
  return `used ${name}`;
}

/** One line for the transcript's end, or null when it looks fine. */
export function stuckLine(stuck: Stuck | null | undefined): string | null {
  if (!stuck) return null;
  switch (stuck.kind) {
    case "repeating":
      return `It may be going in circles: it ${what(stuck)} ${stuck.times} times with the same result.`;
    case "failing":
      return `It may be stuck: its last ${stuck.times} steps failed.`;
    case "silent":
      return `Nothing new for ${stuck.minutes} minutes. A long command may still run, or it may be stuck.`;
  }
}

/** The banner over the task list for sessions Ovra's quit cut off, or null without any. */
export function interruptedLine(count: number): string | null {
  if (count === 0) return null;
  return count === 1 ? "1 agent stopped when Ovra closed." : `${count} agents stopped when Ovra closed.`;
}
