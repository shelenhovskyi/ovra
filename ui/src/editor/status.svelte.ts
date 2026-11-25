// Quiet facts about the shown file for the status bar: where the cursor is,
// the language, the indentation and the line endings. The editor (with its
// cursor) or the text viewer (without one) publishes them; FileStatus.svelte
// shows them. Ovra reads UTF-8 text only, so the encoding is never shown.

import type { EditorState } from "@codemirror/state";

export type Cursor = { line: number; column: number; selected: number };

export type FileFacts = {
  /** Null in the read-only viewer, which has no cursor. */
  cursor: Cursor | null;
  language: string;
  /** "Tab" or "Spaces: N". */
  indent: string;
  lineEnding: "LF" | "CRLF";
};

/** What the editor inserts when nothing in the text says otherwise (CodeMirror's indent unit). */
const DEFAULT_INDENT = "Spaces: 2";
/** Enough indented lines to tell; a large file is not read to the end. */
const SAMPLE_LINES = 2000;

/** The shown file's facts. */
export const fileStatus = $state<{ facts: FileFacts | null }>({ facts: null });
/** Whoever published them (kept out of the state, which would proxy it), so a late clear never wipes the next one's. */
let owner: object | null = null;

export function publishFacts(publisher: object, facts: FileFacts): void {
  owner = publisher;
  fileStatus.facts = facts;
}

export function clearFacts(publisher: object): void {
  if (owner !== publisher) return;
  owner = null;
  fileStatus.facts = null;
}

/** "Ln 12, Col 5", with "(3 selected)" while there is a selection. */
export function cursorLabel({ line, column, selected }: Cursor): string {
  return selected > 0 ? `Ln ${line}, Col ${column} (${selected} selected)` : `Ln ${line}, Col ${column}`;
}

/** The main cursor of `state`: 1-based line and column, and how many characters are selected. */
export function cursorOf(state: EditorState): Cursor {
  const { head, from, to } = state.selection.main;
  const line = state.doc.lineAt(head);
  return { line: line.number, column: head - line.from + 1, selected: to - from };
}

/** "LF", or "CRLF" when the text's first line break is one. */
export function lineEndingOf(text: string): "LF" | "CRLF" {
  const at = text.indexOf("\n");
  return at > 0 && text[at - 1] === "\r" ? "CRLF" : "LF";
}

/**
 * The indentation the text uses: "Tab" when most indented lines start with a
 * tab, else "Spaces: N" for the most common step between indented lines.
 */
export function detectIndent(lines: Iterable<string>): string {
  let tabs = 0;
  let spaced = 0;
  let previous = 0;
  const steps = new Map<number, number>();
  let seen = 0;
  for (const line of lines) {
    if (++seen > SAMPLE_LINES) break;
    const content = line.trimStart();
    if (content === "") continue;
    const lead = line.slice(0, line.length - content.length);
    if (lead.startsWith("\t")) {
      tabs++;
      continue;
    }
    // A block comment's " * " lines are not a step.
    if (content.startsWith("*")) continue;
    const width = lead.length;
    if (width > 0) spaced++;
    const step = Math.abs(width - previous);
    if (step > 0 && step <= 8) steps.set(step, (steps.get(step) ?? 0) + 1);
    previous = width;
  }
  if (tabs > spaced) return "Tab";
  let best = 0;
  let count = 0;
  for (const [step, n] of steps) {
    if (n > count || (n === count && step < best)) [best, count] = [step, n];
  }
  return best > 0 ? `Spaces: ${best}` : DEFAULT_INDENT;
}
