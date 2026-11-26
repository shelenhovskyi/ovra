// Copy and paste between a terminal and the system clipboard.

import type { Terminal } from "@xterm/xterm";

type Pane = Pick<Terminal, "hasSelection" | "getSelection" | "paste">;

function clipboard(): Clipboard | null {
  return typeof navigator !== "undefined" && navigator.clipboard ? navigator.clipboard : null;
}

/** Copies the selection; false when there is none. */
export function copySelection(t: Pane): boolean {
  if (!t.hasSelection()) return false;
  clipboard()
    ?.writeText(t.getSelection())
    .catch(() => {});
  return true;
}

/** Types the clipboard's text into the shell, as a paste (bracketed when the shell asks for it). */
export function pasteClipboard(t: Pane): void {
  clipboard()
    ?.readText()
    .then((text) => {
      if (text) t.paste(text);
    })
    // The browser may refuse to read the clipboard; the system's paste still works.
    .catch(() => {});
}
