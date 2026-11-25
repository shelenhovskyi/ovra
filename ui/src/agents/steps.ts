// What a working session does right now, in a few words, for the live row at
// the end of its transcript.

import { describeTool } from "../approvals/describe";
import type { AgentMessage, AgentStatus } from "../lib/protocol";

/** The first word of a tool's summary, while it runs. */
const NOW: Record<string, string> = {
  Run: "Running:",
  Edit: "Editing",
  Write: "Writing",
  Read: "Reading",
  Search: "Searching",
  Find: "Finding",
  Fetch: "Fetching",
  Start: "Starting",
  Message: "Messaging",
  Stop: "Stopping",
  List: "Listing",
  Ask: "Asking",
  Show: "Showing",
  Use: "Using",
};

/** A step's summary without what follows a colon (a worker's task, a review summary). */
function short(tool: string, action: string): string {
  const colon = action.indexOf(": ");
  return colon < 0 || tool === "Bash" ? action : action.slice(0, colon);
}

function reword(action: string, words: Record<string, string>): string {
  const space = action.indexOf(" ");
  const first = space < 0 ? action : action.slice(0, space);
  const word = words[first];
  return word ? `${word}${action.slice(first.length)}` : action;
}

/**
 * What a session that works does right now: the tool it runs, writing,
 * thinking or starting; `null` when it waits for someone.
 */
export function liveStep(status: AgentStatus, messages: AgentMessage[], writing: boolean): string | null {
  if (status.state === "starting") return "Starting";
  if (status.state !== "working") return null;
  // Only this turn's calls: one an interrupt cut off never gets a result.
  const done = new Set<string>();
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (!m || m.kind === "turn_end" || m.kind === "user_text") break;
    if (m.kind === "tool_result") done.add(m.tool_use_id);
    else if (m.kind === "tool_use" && !done.has(m.id))
      return reword(short(m.name, describeTool(m.name, m.input).action), NOW);
  }
  return writing ? "Writing" : "Thinking";
}

/** `4s`, `2m 05s`. */
export function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}
