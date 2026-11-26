// What a terminal offers an agent task (docs/specs/task-context.md): its
// selection, else its last lines, where a failed test or build prints.

import type { Terminal } from "@xterm/xterm";

/** Lines taken when nothing is selected. */
const LAST_LINES = 80;
/** What the core takes of one text item, in UTF-8 bytes. */
const MAX_BYTES = 16 * 1024;

/** The end of `text` that fits in `bytes` of UTF-8, from a line start when it is cut. */
export function tailBytes(text: string, bytes = MAX_BYTES): string {
  const encoder = new TextEncoder();
  if (encoder.encode(text).length <= bytes) return text;
  const lines = text.split("\n");
  const kept: string[] = [];
  let size = 0;
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i] ?? "";
    size += encoder.encode(line).length + 1;
    if (size > bytes) break;
    kept.unshift(line);
  }
  return kept.join("\n");
}

/** The terminal's selection, else its last non-empty lines; "" when it shows nothing. */
export function terminalText(terminal: Pick<Terminal, "getSelection" | "buffer">): { text: string; selected: boolean } {
  const selection = terminal.getSelection();
  if (selection.trim()) return { text: tailBytes(selection), selected: true };
  const buffer = terminal.buffer.active;
  const lines: string[] = [];
  for (let i = Math.max(0, buffer.length - LAST_LINES * 2); i < buffer.length; i++)
    lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
  while (lines.length > 0 && !lines.at(-1)?.trim()) lines.pop();
  return { text: tailBytes(lines.slice(-LAST_LINES).join("\n")), selected: false };
}
