// A transcript as people read it: what they wrote, what Claude said and what
// needs them stay in full; tool calls fold into one-line steps, and steps in
// a row into one group.

import { describeTool } from "../approvals/describe";
import type { AgentMessage, AgentStatus } from "../lib/protocol";
import type { IconName } from "../shell/icons";

type ToolUse = Extract<AgentMessage, { kind: "tool_use" }>;
type ToolResult = Extract<AgentMessage, { kind: "tool_result" }>;

/** One tool call and its result, either of which may be missing. */
export type Step = { use: ToolUse | null; result: ToolResult | null };

type Entry = { kind: "message"; message: AgentMessage } | { kind: "steps"; steps: Step[] };

/** Messages in order, with each tool call and its result as one step and steps in a row together. */
export function entries(messages: AgentMessage[]): Entry[] {
  const out: Entry[] = [];
  const open = new Map<string, Step>();
  for (const message of messages) {
    if (message.kind !== "tool_use" && message.kind !== "tool_result") {
      out.push({ kind: "message", message });
      continue;
    }
    const pending = message.kind === "tool_result" ? open.get(message.tool_use_id) : undefined;
    if (pending && message.kind === "tool_result") {
      pending.result = message;
      continue;
    }
    const step: Step = message.kind === "tool_use" ? { use: message, result: null } : { use: null, result: message };
    if (message.kind === "tool_use") open.set(message.id, step);
    const last = out.at(-1);
    if (last?.kind === "steps") last.steps.push(step);
    else out.push({ kind: "steps", steps: [step] });
  }
  return out;
}

/** The first word of a tool's summary, as done. */
const PAST: Record<string, string> = {
  Run: "Ran",
  Edit: "Edited",
  Write: "Wrote",
  Read: "Read",
  Search: "Searched",
  Find: "Found",
  Fetch: "Fetched",
  Start: "Started",
  Message: "Messaged",
  Stop: "Stopped",
  List: "Listed",
  Ask: "Asked",
  Show: "Showed",
  Use: "Used",
};

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
 * A step in a few words: `Ran npm test`, `Started worker docs`. `action` is
 * its call's summary (`describeTool`).
 */
export function stepLabel(step: Step, action: string): string {
  if (!step.use) return step.result?.is_error ? "Tool failed" : "Tool finished";
  const few = short(step.use.name, action);
  // A call that failed did not do what it says: no past tense.
  return step.result && !step.result.is_error ? reword(few, PAST) : few;
}

export function stepIcon(step: Step): IconName {
  switch (step.use?.name) {
    case "Bash":
      return "terminal";
    case "Edit":
    case "MultiEdit":
    case "NotebookEdit":
    case "Write":
    case "Read":
      return "files";
    case "Glob":
    case "Grep":
    case "WebSearch":
      return "search";
    case "Agent":
    case "Task":
      return "agent";
    default:
      return step.use?.name.startsWith("mcp__ovra__") ? "agent" : "more";
  }
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
