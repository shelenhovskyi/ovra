// Workspace state shared by panels. Changing the shape of this class is a
// shared hot spot (see CLAUDE.md, "Parallel work"): do it in its own commit.

import type { Project } from "../lib/protocol";

export type MainMode = "view" | "edit";

export class Workspace {
  /** The project shown in the window, or null on the start screen. */
  project = $state<Project | null>(null);
  /** Project-relative path of the file in the main area. */
  openPath = $state<string | null>(null);
  /** Whether the main area shows a viewer or the editor for `openPath`. */
  mode = $state<MainMode>("view");
  paletteOpen = $state(false);

  openFile(path: string, mode: MainMode = "view"): void {
    this.openPath = path;
    this.mode = mode;
  }
}
