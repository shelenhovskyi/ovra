// Find, replace and go to line in the shown editor: CodeMirror's own panels,
// reachable from the palette and the context menu, not only from its keys.

import { gotoLine, openSearchPanel } from "@codemirror/search";
import type { EditorView } from "@codemirror/view";

export type SearchAction = "find" | "replace" | "go_to_line";

/** The view the editor shows, set by `Editor.svelte`. */
let shown: EditorView | null = null;

export function showView(view: EditorView | null): void {
  shown = view;
}

/** Opens the search panel (with the replace field focused for "replace") or the go-to-line field. */
export function runSearch(action: SearchAction, view: EditorView | null = shown): void {
  if (!view) return;
  if (action === "go_to_line") {
    gotoLine(view);
    return;
  }
  openSearchPanel(view);
  if (action === "replace") {
    const field = view.dom.querySelector<HTMLInputElement>(".cm-search input[name=replace]");
    field?.focus();
    field?.select();
  }
}
