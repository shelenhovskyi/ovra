// The tree's context menus (docs/design/components.md): find your way first,
// then change, Move to Trash last. Every item is also a palette command
// (`tree.*`) or a row key.

import type { MenuEntry, MenuItem } from "../menu/context-menu.svelte";
import type { FileActions } from "./actions.svelte";
import { folderFor } from "./commands";
import type { Row } from "./model";
import { revealLabel, trashKeys } from "./ops";

/** Cut and Copy of `paths`, and Paste into `dir` when something was cut or copied. */
function clipItems(actions: FileActions, paths: string[], dir: string | null): MenuItem[] {
  const items: MenuItem[] = [
    { label: "Cut", keys: "Mod+X", run: () => actions.cut(paths) },
    { label: "Copy", keys: "Mod+C", run: () => actions.copy(paths) },
  ];
  if (dir !== null) items.push(pasteItem(actions, dir));
  return items;
}

function pasteItem(actions: FileActions, dir: string): MenuItem {
  return { label: "Paste", keys: "Mod+V", disabled: actions.clip === null, run: () => void actions.paste(dir) };
}

/** The menu of a file or folder row. */
export function rowMenu(actions: FileActions, row: Pick<Row, "path" | "kind">): MenuEntry[] {
  const dir = folderFor(row);
  return [
    { label: revealLabel(), run: () => actions.reveal(row.path) },
    { label: "Copy path", run: () => actions.copyPaths([row.path], false) },
    { label: "Copy relative path", run: () => actions.copyPaths([row.path], true) },
    "separator",
    ...clipItems(actions, [row.path], dir),
    "separator",
    { label: "New file…", run: () => actions.startNew(dir, "file") },
    { label: "New folder…", run: () => actions.startNew(dir, "dir") },
    { label: "Rename…", keys: "F2", run: () => actions.startRename(row.path) },
    { label: "Duplicate", run: () => actions.duplicate(row.path) },
    "separator",
    { label: "Move to Trash…", keys: trashKeys(), danger: true, run: () => actions.askTrash([row.path]) },
  ];
}

/** The menu of several selected rows: what works on all of them at once. */
export function selectionMenu(actions: FileActions, paths: string[]): MenuEntry[] {
  return [
    { label: "Copy paths", run: () => actions.copyPaths(paths, false) },
    { label: "Copy relative paths", run: () => actions.copyPaths(paths, true) },
    "separator",
    ...clipItems(actions, paths, null),
    "separator",
    { label: "Move to Trash…", keys: trashKeys(), danger: true, run: () => actions.askTrash(paths) },
  ];
}

/** The menu of the space below the rows and of the header: the project folder. */
export function rootMenu(actions: FileActions, collapseAll: () => void): MenuEntry[] {
  return [
    { label: "Reveal project folder", run: () => actions.reveal("") },
    { label: "Collapse all", run: collapseAll },
    "separator",
    { label: "New file…", run: () => actions.startNew("", "file") },
    { label: "New folder…", run: () => actions.startNew("", "dir") },
    pasteItem(actions, ""),
  ];
}
