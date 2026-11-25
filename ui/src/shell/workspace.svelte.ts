// Workspace state shared by panels. Changing the shape of this class is a
// shared hot spot (see CLAUDE.md, "Parallel work"): do it in its own commit.

import type { Project } from "../lib/protocol";

export type MainMode = "view" | "edit";

/** What the left sidebar shows. */
export type SidebarView = "files" | "search" | "git";

/** A place in a file to show and select: `from` and `to` are UTF-16 offsets in line `line` (from 1). */
export type TextSpot = { line: number; from: number; to: number };

export class Workspace {
  /** The project shown in the window, or null on the start screen. */
  project = $state<Project | null>(null);
  /** Project-relative path of the file in the main area. */
  openPath = $state<string | null>(null);
  /** Whether the main area shows a viewer or the editor for `openPath`. */
  mode = $state<MainMode>("view");
  paletteOpen = $state(false);
  sidebar = $state<SidebarView>("files");
  /** A spot the editor should select once it shows `path`; it clears this when done. */
  reveal = $state<(TextSpot & { path: string }) | null>(null);

  /** Opens `path`; with `at`, in the editor with that spot selected. */
  openFile(path: string, mode: MainMode = "view", at?: TextSpot): void {
    this.openPath = path;
    this.mode = at ? "edit" : mode;
    this.reveal = at ? { path, ...at } : null;
  }
}
