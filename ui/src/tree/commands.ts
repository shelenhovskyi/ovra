// The tree's file actions as palette commands. They act on the active row
// (a folder's new file goes inside it, a file's next to it) and have no global
// keys: F2 and Delete belong to the focused tree, the editor has its own F2.

import { registerCommand } from "../keys/commands";
import type { FileActions } from "./actions.svelte";
import { parentOf, type Row } from "./model";
import { revealLabel } from "./ops";

/** The folder a new entry goes in when `row` is active (`""` is the project folder). */
export function folderFor(row: Pick<Row, "path" | "kind"> | null): string {
  if (!row) return "";
  return row.kind === "dir" ? row.path : parentOf(row.path);
}

/** Registers the commands; `active` is the active real row, `show` makes the tree visible first. */
export function registerFileCommands(
  actions: FileActions,
  active: () => Pick<Row, "path" | "kind"> | null,
  show: () => void,
): () => void {
  const on = (run: (row: Pick<Row, "path" | "kind">) => unknown) => () => {
    const row = active();
    if (!row) return;
    show();
    void run(row);
  };
  const create = (entry: "file" | "dir") => () => {
    show();
    void actions.startNew(folderFor(active()), entry);
  };
  const unregister = [
    registerCommand({ id: "tree.new_file", title: "New file…", run: create("file") }),
    registerCommand({ id: "tree.new_folder", title: "New folder…", run: create("dir") }),
    registerCommand({ id: "tree.rename", title: "Rename file…", run: on((row) => actions.startRename(row.path)) }),
    registerCommand({ id: "tree.duplicate", title: "Duplicate file", run: on((row) => actions.duplicate(row.path)) }),
    registerCommand({ id: "tree.trash", title: "Move file to Trash", run: on((row) => actions.askTrash([row.path])) }),
    registerCommand({
      id: "tree.copy_path",
      title: "Copy file path",
      run: on((row) => actions.copyPaths([row.path], false)),
    }),
    registerCommand({
      id: "tree.copy_relative_path",
      title: "Copy relative file path",
      run: on((row) => actions.copyPaths([row.path], true)),
    }),
    registerCommand({
      id: "tree.reveal_in_os",
      title: revealLabel(),
      run: () => void actions.reveal(active()?.path ?? ""),
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
