// A task's outcome in three lines (done, checked, left), read from the
// agent's last message or its review summary. Ovra asks agents to end with
// these lines (`agents/roles/task.md`, `worker.md`); text without them has
// no summary, and the UI shows the agent's own words as before.

import type { AgentMessage } from "../lib/protocol";
import { reportOf } from "./report";

/** What the agent says it did, how it checked it and what is left; a line it left out is null. */
export type TaskSummary = { done: string; checked: string | null; left: string | null };

const LABELS: [keyof TaskSummary, RegExp][] = [
  ["done", /^(done|changed|did)$/],
  ["checked", /^(checked|verified|tested)$/],
  ["left", /^(left|remaining|not done|left to do|todo|to do|next)$/],
];

/** One `Label: text` line, once list marks, headings and bold are taken off. */
const LINE = /^([A-Za-z][A-Za-z ]{1,14}?)\s*:\s*(.*)$/;

/** The summary in `text`: the last `Done:` line and the `Checked:` and `Left:` lines after it. */
export function taskSummary(text: string | null | undefined): TaskSummary | null {
  if (!text) return null;
  const lines = text.split("\n");
  let found: TaskSummary | null = null;
  for (const line of lines) {
    const m = LINE.exec(
      line
        .trim()
        .replace(/^([-*+]|#+)\s+/, "")
        .replace(/\*\*|__/g, ""),
    );
    if (!m) continue;
    const label = (m[1] ?? "").trim().toLowerCase();
    const value = clean(m[2] ?? "");
    const key = LABELS.find(([, re]) => re.test(label))?.[0];
    if (!key || !value) continue;
    // A later Done line starts a new summary: the last one is the answer.
    if (key === "done") found = { done: value, checked: null, left: null };
    else if (found) found[key] = value;
  }
  return found;
}

/** The line, cut to one readable sentence's length. */
function clean(value: string): string {
  const plain = value.trim();
  return plain.length > 240 ? `${plain.slice(0, 239).trimEnd()}…` : plain;
}

/** Whether what is left reads as nothing, so the line can say so calmly. */
export function nothingLeft(left: string | null): boolean {
  return left !== null && /^(nothing|none|no|n\/a|-)\.?$/i.test(left.trim());
}

/** A session's summary: from its review's summary, else from its last answer. */
export function sessionSummary(
  review: { summary: string | null } | undefined,
  messages: readonly AgentMessage[] | undefined,
): TaskSummary | null {
  return taskSummary(review?.summary) ?? taskSummary(reportOf(messages));
}
