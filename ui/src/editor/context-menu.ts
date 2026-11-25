// The editor's context menu: clipboard, search, language features while a
// server serves the file, and the file's path.

import { selectAll } from "@codemirror/commands";
import { LSPPlugin } from "@codemirror/lsp-client";
import type { EditorView } from "@codemirror/view";
import { commandItem, type MenuEntry } from "../menu/context-menu.svelte";
import { runSearch } from "./search";

/** `path` under the project's `root`, written the way the root is (backslashes on Windows). */
export function absolutePath(root: string, path: string): string {
  const windows = root.includes("\\") && !root.includes("/");
  const separator = windows ? "\\" : "/";
  const base = root.endsWith(separator) ? root.slice(0, -1) : root;
  return `${base}${separator}${windows ? path.replaceAll("/", "\\") : path}`;
}

function clipboard(): Clipboard | null {
  return typeof navigator !== "undefined" && navigator.clipboard ? navigator.clipboard : null;
}

/** Copy path, Copy relative path and Reveal in file tree, shared by the editor and the viewer. */
export function pathEntries(root: string | null, path: string): MenuEntry[] {
  const board = clipboard();
  return [
    board && root !== null && { label: "Copy path", run: () => board.writeText(absolutePath(root, path)) },
    board && { label: "Copy relative path", run: () => board.writeText(path) },
    commandItem("tree.reveal", "Reveal in file tree"),
  ];
}

export function editorMenu(view: EditorView, root: string | null, path: string): MenuEntry[] {
  const board = clipboard();
  const { state } = view;
  const selected = () => state.sliceDoc(state.selection.main.from, state.selection.main.to);
  const empty = state.selection.main.empty;
  const lsp = LSPPlugin.get(view) !== null;
  const search = (action: Parameters<typeof runSearch>[0]) => () => runSearch(action, view);
  return [
    // Items that do not apply are left out (docs/design/components.md).
    board &&
      !empty &&
      !state.readOnly && {
        label: "Cut",
        keys: "Mod+X",
        run: async () => {
          await board.writeText(selected());
          view.dispatch(view.state.replaceSelection(""), { userEvent: "delete.cut", scrollIntoView: true });
        },
      },
    board && !empty && { label: "Copy", keys: "Mod+C", run: () => board.writeText(selected()) },
    // Reading the clipboard needs a browser that allows it; the menu leaves Paste out otherwise.
    typeof board?.readText === "function" &&
      !state.readOnly && {
        label: "Paste",
        keys: "Mod+V",
        run: async () => {
          const text = await board.readText();
          view.dispatch(view.state.replaceSelection(text), { userEvent: "input.paste", scrollIntoView: true });
        },
      },
    { label: "Select all", keys: "Mod+A", run: () => void selectAll(view) },
    "separator",
    { label: "Find", keys: "Mod+F", run: search("find") },
    { label: "Replace", keys: "Mod+Alt+F", run: search("replace") },
    { label: "Go to line", keys: "Ctrl+G", run: search("go_to_line") },
    "separator",
    lsp && commandItem("editor.lsp_definition", "Go to definition"),
    lsp && commandItem("editor.lsp_references", "Find references"),
    lsp && commandItem("editor.lsp_rename", "Rename symbol"),
    "separator",
    ...pathEntries(root, path),
  ];
}
