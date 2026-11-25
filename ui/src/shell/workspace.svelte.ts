// Workspace state shared by panels. Changing the shape of this class is a
// shared hot spot (see CLAUDE.md, "Parallel work"): do it in its own commit.

import type { Project, ProjectId } from "../lib/protocol";

export type MainMode = "view" | "edit";

/** What the left sidebar shows. */
export type SidebarView = "files" | "search" | "git";

/** A place in a file to show and select: `from` and `to` are UTF-16 offsets in line `line` (from 1). */
export type TextSpot = { line: number; from: number; to: number };

/** A file's git diff in the main area: index to working tree, or with `staged` HEAD to index. */
export type GitDiffTarget = { project: ProjectId; path: string; staged: boolean };

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
  /** A diff the main area shows instead of `openPath`; opening a file closes it. */
  gitDiff = $state<GitDiffTarget | null>(null);

  /** Opens `path`; with `at`, in the editor with that spot selected. */
  openFile(path: string, mode: MainMode = "view", at?: TextSpot): void {
    this.gitDiff = null;
    this.openPath = path;
    this.mode = at ? "edit" : mode;
    this.reveal = at ? { path, ...at } : null;
  }
}
