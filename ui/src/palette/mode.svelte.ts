// Which list the palette shows when it opens. Kept here rather than in the
// workspace state: only the palette and its commands care.

import type { Workspace } from "../shell/workspace.svelte";
import type { Source } from "./sources";

/**
 * `search` is "Search or run": files (projects without one open) and commands.
 * `folder` asks for the path of a folder to open, keeping the project open meanwhile.
 * `shortcuts` lists the commands that have a shortcut.
 * `list` shows a list another area hands over with `showList`.
 */
export type PaletteMode = "search" | "commands" | "files" | "projects" | "folder" | "shortcuts" | "list";

export const palette = $state<{ mode: PaletteMode; list: Source | null }>({ mode: "commands", list: null });

/**
 * Opens the palette on `list`, such as the symbols of the open file: fuzzy
 * filtered by label like every mode, Enter runs the entry. Give the source
 * an `idle` to choose what an empty query shows (such as every entry in order).
 */
export function showList(list: Source, workspace: Pick<Workspace, "paletteOpen">): void {
  palette.list = list;
  palette.mode = "list";
  workspace.paletteOpen = true;
}
