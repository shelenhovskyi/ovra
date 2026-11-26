// A transcript as people read it: what they wrote, what Claude said and what
// needs them stay in full; each tool call is one step with its result, and
// reads and searches in a row fold into one group. Edits, commands and
// failures always keep a step of their own.

import { describeTool } from "../approvals/describe";
import type { AgentMessage, AgentStatus, Billing, TurnUsage } from "../lib/protocol";
import type { IconName } from "../shell/icons";

type ToolUse = Extract<AgentMessage, { kind: "tool_use" }>;
type ToolResult = Extract<AgentMessage, { kind: "tool_result" }>;

/** One tool call and its result, either of which may be missing. */
export type Step = { use: ToolUse | null; result: ToolResult | null };

/** Ovra's note to an agent: its first line is for the person, the rest (ids, tool hints, a diff) for the agent. */
export type Note = { line: string; rest: string };

type Entry =
  | { kind: "message"; message: AgentMessage }
  | { kind: "steps"; steps: Step[] }
  /** Ovra's notes in a row: one quiet line. `first`: the first note's message. */
  | { kind: "notes"; notes: Note[]; first: AgentMessage };

/** How the core's message with the person's @ context begins (crates/core/src/context.rs). */
const CONTEXT_HEADER = "The person added this context to the message.";

/** The items of a context message as the person added them, without the wording meant for the agent. */
function contextItems(text: string): string {
  const body = text.slice(text.indexOf("\n\n") + 2);
  return body
    .replace(/<untrusted origin="[^"]*">/g, "")
    .replace(/<\/untrusted>/g, "")
    .replaceAll("&lt;", "<")
    .trim();
}

function note(text: string): Note {
  // The person knows what they added: one line, the items folded under it.
  if (text.startsWith(CONTEXT_HEADER)) return { line: "Context sent", rest: contextItems(text) };
  const plain = text.startsWith("Ovra: ") ? text.slice("Ovra: ".length) : text;
  const end = plain.indexOf("\n");
  return end < 0 ? { line: plain, rest: "" } : { line: plain.slice(0, end), rest: plain.slice(end + 1).trim() };
}

const named = (names: string[]): string =>
  names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;

/** What the person did to a worker's work, as Ovra tells the coordinator, and how the person is told. */
const BY_PERSON: [RegExp, string][] = [
  [/^had its work approved by the person, except /, "You kept part of"],
  [/^had its work approved by the person/, "You kept"],
  [/^had its work rejected by the person/, "You discarded"],
];

type Said = { name: string; what: string; you: string | null };

/** A note about one worker: its name, what it did, or what the person did ("You kept"). */
function said(line: string): Said | null {
  const match = /^Worker (\S+) (.+)$/.exec(line.replace(/\.$/, ""));
  if (!match) return null;
  const [, name = "", what = ""] = match;
  return { name, what, you: BY_PERSON.find(([pattern]) => pattern.test(what))?.[1] ?? null };
}

const capital = (text: string): string => text.charAt(0).toUpperCase() + text.slice(1);

function sentence(names: string[], what: Said): string {
  if (what.you) return `${what.you} ${named(names)}`;
  return `${names.length > 1 ? "Workers" : "Worker"} ${named(names)} ${what.what}`;
}

/** One note's line in the person's words: "You kept lint", not "Worker lint had its work approved by the person". */
export function noteLine(line: string): string {
  const what = said(line);
  return what ? sentence([what.name], what) : capital(line.replace(/\.$/, ""));
}

/**
 * Ovra's notes in a row as one line: notes in a row that say the same of
 * several workers become one (`Workers docs, tests and lint asked for
 * review · You kept lint`), the others keep their own line.
 */
export function notesLabel(notes: Note[]): string {
  const runs: { what: Said | null; names: string[]; line: string }[] = [];
  for (const { line } of notes) {
    const what = said(line);
    const last = runs.at(-1);
    if (what && last?.what && (last.what.you ?? last.what.what) === (what.you ?? what.what)) {
      if (!last.names.includes(what.name)) last.names.push(what.name);
    } else runs.push({ what, names: what ? [what.name] : [], line });
  }
  return runs.map((run) => (run.what ? sentence(run.names, run.what) : noteLine(run.line))).join(" · ");
}

/** Tools that only look: in a row they fold into one line. */
const LOOKS = new Set(["Read", "Glob", "Grep", "WebSearch", "LS"]);
const looks = (step: Step): boolean =>
  step.use !== null && LOOKS.has(step.use.name) && !step.result?.is_error && !step.use.parent;

/**
 * Messages in order, each tool call merged with its result (by
 * `tool_use_id`); reads and searches in a row together, and Ovra's notes in
 * a row together.
 */
export function entries(messages: AgentMessage[]): Entry[] {
  const flat: ({ kind: "message"; message: AgentMessage } | { kind: "step"; step: Step })[] = [];
  const open = new Map<string, Step>();
  for (const message of messages) {
    if (message.kind !== "tool_use" && message.kind !== "tool_result") {
      flat.push({ kind: "message", message });
      continue;
    }
    const pending = message.kind === "tool_result" ? open.get(message.tool_use_id) : undefined;
    if (pending && message.kind === "tool_result") {
      pending.result = message;
      continue;
    }
    const step: Step = message.kind === "tool_use" ? { use: message, result: null } : { use: null, result: message };
    if (message.kind === "tool_use") open.set(message.id, step);
    flat.push({ kind: "step", step });
  }
  // Only now are results known: a read that failed keeps its own step.
  const out: Entry[] = [];
  for (const item of flat) {
    const last = out.at(-1);
    if (item.kind === "message" && item.message.kind === "user_text" && item.message.from === "ovra") {
      if (last?.kind === "notes") last.notes.push(note(item.message.text));
      else out.push({ kind: "notes", notes: [note(item.message.text)], first: item.message });
      continue;
    }
    if (item.kind === "message") {
      out.push(item);
      continue;
    }
    if (looks(item.step) && last?.kind === "steps" && last.steps.every(looks)) last.steps.push(item.step);
    else out.push({ kind: "steps", steps: [item.step] });
  }
  return out;
}

/** `Read 6 files, searched 2 times`, for a fold of reads and searches. */
export function foldLabel(steps: Step[]): string {
  const reads = steps.filter((s) => s.use?.name === "Read").length;
  const searches = steps.length - reads;
  const parts = [
    reads > 0 ? `read ${reads} ${reads === 1 ? "file" : "files"}` : "",
    searches > 0 ? `searched ${searches === 1 ? "once" : `${searches} times`}` : "",
  ].filter(Boolean);
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** A step's label as a verb and the thing it acted on (`Ran` + `npm test`); the object is empty for other tools. */
export function verbAndObject(step: Step, label: string): { verb: string; object: string } {
  const plain = { verb: label, object: "" };
  if (
    !step.use ||
    !["Bash", "Edit", "MultiEdit", "NotebookEdit", "Write", "Read", "Glob", "Grep", "WebFetch"].includes(step.use.name)
  )
    return plain;
  const two = /^(Found|Find) files /.exec(label);
  const at = two ? two[0].length - 1 : label.indexOf(" ");
  return at < 0 ? plain : { verb: label.slice(0, at), object: label.slice(at + 1) };
}

const lineCount = (text: unknown): number => (typeof text === "string" && text !== "" ? text.split("\n").length : 0);

/** A step's numbers, right-aligned on its line: lines added and removed by an edit or a write. */
export function stepMeta(step: Step): string {
  const input = (step.use?.input ?? {}) as Record<string, unknown>;
  switch (step.use?.name) {
    case "Edit":
      return `+${lineCount(input.new_string)} −${lineCount(input.old_string)}`;
    case "Write":
      return `+${lineCount(input.content)}`;
    default:
      return "";
  }
}

/**
 * What a turn cost, in a few words for its end line: tokens on a Claude
 * plan (where dollars are only an estimate), dollars on an API key or a
 * cloud account.
 */
export function turnCost(usage: TurnUsage, billing: Billing | null): string {
  const paid = billing !== null && billing.kind !== "subscription" && usage.cost_micro_usd !== null;
  if (paid) {
    const dollars = (usage.cost_micro_usd ?? 0) / 1_000_000;
    return dollars < 0.01 ? "under $0.01" : `$${dollars.toFixed(2)}`;
  }
  const tokens = usage.input_tokens + usage.output_tokens;
  return tokens < 1000 ? `${tokens} tokens` : `${(tokens / 1000).toFixed(1)}k tokens`;
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
export function liveStep(
  status: AgentStatus,
  messages: AgentMessage[],
  writing: boolean,
  root: string | null = null,
): string | null {
  if (status.state === "starting") return "Starting";
  if (status.state !== "working") return null;
  // Only this turn's calls: one an interrupt cut off never gets a result.
  const done = new Set<string>();
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (!m || m.kind === "turn_end" || m.kind === "user_text") break;
    if (m.kind === "tool_result") done.add(m.tool_use_id);
    else if (m.kind === "tool_use" && !done.has(m.id))
      return reword(short(m.name, describeTool(m.name, inFolder(m.input, root)).action), NOW);
  }
  return writing ? "Writing" : "Thinking";
}

/** The input fields that name a file the tool works on. */
const PATH_FIELDS = ["file_path", "notebook_path"];

/**
 * A tool call's input with the file it works on relative to `root`, the
 * session's folder (the project, or a worker's copy): Claude sends absolute
 * paths, whose start says nothing and pushes the file name out of sight.
 */
export function inFolder(input: unknown, root: string | null): unknown {
  if (!root || typeof input !== "object" || input === null) return input;
  const base = root.endsWith("/") ? root : `${root}/`;
  const fields = input as Record<string, unknown>;
  const out = { ...fields };
  for (const key of PATH_FIELDS) {
    const path = fields[key];
    if (typeof path === "string" && path.startsWith(base) && path.length > base.length)
      out[key] = path.slice(base.length);
  }
  return out;
}

/** `4s`, `2m 05s`. */
export function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}
