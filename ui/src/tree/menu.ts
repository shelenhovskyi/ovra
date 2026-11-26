// The tree's context menus (docs/design/components.md): at most 3 groups and
// about 10 items, in VS Code's order. Create and find your way first, then
// change, Move to Trash last. Every item is also a palette command (`tree.*`)
// or a row key; what a menu leaves out (Copy relative path without Alt,
// New file… on a file) stays in the palette.

import type { MenuEntry, MenuItem } from "../menu/context-menu.svelte";
import { findInFolder } from "../search/find-in";
import { openTerminalIn } from "../terminal/open-in";
import type { FileActions } from "./actions.svelte";
import { addToTask, folderFor } from "./commands";
import { isGone, type Row } from "./model";
import { revealLabel, trashKeys } from "./ops";

/** New file… and New folder… in `dir`. */
function newItems(actions: FileActions, dir: string): MenuItem[] {
  return [
    { label: "New file…", run: () => actions.startNew(dir, "file") },
    { label: "New folder…", run: () => actions.startNew(dir, "dir") },
  ];
}

/** Paste into `dir`, only when something was cut or copied. */
function pasteItem(actions: FileActions, dir: string): MenuItem | null {
  return actions.clip === null ? null : { label: "Paste", keys: "Mod+V", run: () => void actions.paste(dir) };
}

/** Copy path of `paths`; with Alt (Option) held, their paths relative to the project. */
function copyPathItem(actions: FileActions, paths: string[]): MenuItem {
  const many = paths.length > 1;
  return {
    label: many ? "Copy paths" : "Copy path",
    run: () => actions.copyPaths(paths, false),
    alt: { label: many ? "Copy relative paths" : "Copy relative path", run: () => actions.copyPaths(paths, true) },
  };
}

/** Add to task for the rows at `paths`; `isDir` tells folders from files. */
function taskItem(paths: string[], isDir: (path: string) => boolean): MenuItem {
  return { label: "Add to task", run: () => addToTask(paths.map((path) => ({ path, dir: isDir(path) }))) };
}

/** Opens a terminal in `dir`. */
function terminalItem(dir: string): MenuItem {
  return { label: "Open in terminal", run: () => openTerminalIn(dir) };
}

function trashItem(actions: FileActions, paths: string[]): MenuItem {
  return { label: "Move to Trash…", keys: trashKeys(), danger: true, run: () => actions.askTrash(paths) };
}

/** The menu of a file or folder row; New file…, New folder… and Find in folder… on folders only. */
export function rowMenu(
  actions: FileActions,
  row: Pick<Row, "path" | "kind"> & Partial<Pick<Row, "git">>,
): MenuEntry[] {
  // A deleted file has only its path left.
  if (isGone({ kind: row.kind, git: row.git ?? null })) return [copyPathItem(actions, [row.path])];
  const dir = folderFor(row);
  const folder = row.kind === "dir";
  return [
    ...(folder ? newItems(actions, dir) : []),
    folder && { label: "Find in folder…", run: () => findInFolder(row.path) },
    { label: revealLabel(), run: () => actions.reveal(row.path) },
    terminalItem(dir),
    taskItem([row.path], () => folder),
    "separator",
    { label: "Cut", keys: "Mod+X", run: () => actions.cut([row.path]) },
    { label: "Copy", keys: "Mod+C", run: () => actions.copy([row.path]) },
    pasteItem(actions, dir),
    copyPathItem(actions, [row.path]),
    { label: "Duplicate", run: () => actions.duplicate(row.path) },
    { label: "Rename…", keys: "F2", run: () => actions.startRename(row.path) },
    "separator",
    trashItem(actions, [row.path]),
  ];
}

/** The menu of several selected rows: what works on all of them at once. */
export function selectionMenu(actions: FileActions, paths: string[], isDir: (path: string) => boolean): MenuEntry[] {
  return [
    taskItem(paths, isDir),
    "separator",
    { label: "Cut", keys: "Mod+X", run: () => actions.cut(paths) },
    { label: "Copy", keys: "Mod+C", run: () => actions.copy(paths) },
    copyPathItem(actions, paths),
    "separator",
    trashItem(actions, paths),
  ];
}

/** The menu of the space below the rows and of the header: the project folder. */
export function rootMenu(actions: FileActions, collapseAll: () => void): MenuEntry[] {
  return [
    ...newItems(actions, ""),
    { label: "Reveal project folder", run: () => actions.reveal("") },
    terminalItem(""),
    "separator",
    pasteItem(actions, ""),
    { label: "Collapse all", run: collapseAll },
  ];
}
