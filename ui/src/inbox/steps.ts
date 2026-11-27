// Which step of the agent's session wrote a part of a file's diff, for the
// line above each part in Reviews. A step is one of the session's own tool
// calls, numbered in the order they ran; the guess compares the part's added
// lines with what each file tool call wrote. Words only: it decides nothing.

import { claudeCall } from "../approvals/claude-calls";
import type { AgentMessage } from "../lib/protocol";
import type { ParsedHunk } from "./hunks";

export type Step = {
  /** From 1, among the session's tool calls (a subagent's calls not counted). */
  number: number;
  /** The tool as the agent named it: Edit, Write… */
  tool: string;
  /** What the agent said just before the call, its first line, shortened. */
  said: string | null;
};

type Candidate = Step & { wrote: string };

const SAID_MAX = 120;

/** The text a file tool call wrote, from its input as the agent sent it. */
function written(input: unknown): string {
  if (!input || typeof input !== "object") return "";
  const fields = input as Record<string, unknown>;
  const parts: string[] = [];
  for (const key of ["new_string", "content", "new_str", "file_text"]) {
    const value = fields[key];
    if (typeof value === "string") parts.push(value);
  }
  if (Array.isArray(fields.edits)) {
    for (const edit of fields.edits) parts.push(written(edit));
  }
  return parts.join("\n");
}

/** Whether `named`, a path as the agent wrote it, is `path` of the project. */
function names(named: string, path: string): boolean {
  const plain = named.replaceAll("\\", "/").replace(/^\.\//, "");
  return plain === path || plain.endsWith(`/${path}`);
}

function shorten(text: string): string | null {
  const line = text.trim().split("\n")[0]?.trim() ?? "";
  if (!line) return null;
  return line.length > SAID_MAX ? `${line.slice(0, SAID_MAX - 1)}…` : line;
}

/** The session's calls that edited `path`, in the order they ran. */
export function fileSteps(messages: readonly AgentMessage[], path: string): Candidate[] {
  const steps: Candidate[] = [];
  let number = 0;
  let said: string | null = null;
  for (const message of messages) {
    if (message.kind === "assistant_text" && !message.parent) said = shorten(message.text);
    if (message.kind !== "tool_use" || message.parent) continue;
    number++;
    // A core from before calls were mapped sends Claude's own names only.
    const call = message.call ?? claudeCall(message.name, message.input);
    if (call?.kind === "edit" && call.paths?.some((p) => names(p, path))) {
      steps.push({ number, tool: message.name, said, wrote: written(message.input) });
    }
    said = null;
  }
  return steps;
}

/**
 * The step that wrote `part`: the last of `steps` whose text holds one of
 * the part's added lines; a part that only removes lines, or one no step's
 * text holds, is the only step's when there is one. Null when unsure.
 */
export function stepOf(part: ParsedHunk, steps: readonly Candidate[]): Step | null {
  const added = part.lines
    .filter((l) => l.kind === "added")
    .map((l) => l.text.trim())
    .filter((t) => t.length > 0);
  for (let i = steps.length - 1; i >= 0; i--) {
    const step = steps[i];
    if (step && added.some((line) => step.wrote.includes(line))) return strip(step);
  }
  const only = steps.length === 1 ? steps[0] : undefined;
  return only ? strip(only) : null;
}

function strip({ number, tool, said }: Candidate): Step {
  return { number, tool, said };
}
