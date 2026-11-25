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

/** An editor group (docs/specs/split-editor.md): 0 on the left, 1 on the right. */
export type GroupIndex = 0 | 1;

/** What an editor group shows. */
export type GroupShown = { openPath: string | null; mode: MainMode };

export class Workspace {
  /** The project shown in the window, or null on the start screen. */
  project = $state<Project | null>(null);
  /** Project-relative path of the file in the focused editor group. */
  openPath = $state<string | null>(null);
  /** Whether the focused group shows a viewer or the editor for `openPath`. */
  mode = $state<MainMode>("view");
  /** The focused editor group; 0 while there is only one. */
  group = $state<GroupIndex>(0);
  /** What the other editor group shows, or null while the main area has one group. */
  other = $state<GroupShown | null>(null);
  paletteOpen = $state(false);
  sidebar = $state<SidebarView>("files");
  /** A spot the editor should select once it shows `path`; it clears this when done. */
  reveal = $state<(TextSpot & { path: string }) | null>(null);
  /** A diff the main area shows instead of `openPath`; opening a file closes it. */
  gitDiff = $state<GitDiffTarget | null>(null);
  /**
   * Whether the panel reading this shows the focused editor group: always
   * true here, and in `groupView` true only for the focused group.
   */
  readonly focused: boolean = true;

  /** Opens `path` in the focused group; with `at`, in the editor with that spot selected. */
  openFile(path: string, mode: MainMode = "view", at?: TextSpot): void {
    this.gitDiff = null;
    this.openPath = path;
    this.mode = at ? "edit" : mode;
    this.reveal = at ? { path, ...at } : null;
  }

  /** Focuses editor group `group`, if it exists: `openPath` and `mode` become its own. */
  focusGroup(group: GroupIndex): void {
    const other = this.other;
    if (group === this.group || !other) return;
    this.other = { openPath: this.openPath, mode: this.mode };
    this.openPath = other.openPath;
    this.mode = other.mode;
    this.group = group;
  }

  /** Adds a group on the right that shows `shown`, and focuses it; with two groups, does nothing. */
  split(shown: GroupShown): void {
    if (this.other) return;
    this.other = shown;
    this.focusGroup(1);
  }

  /** Closes editor group `group`; the one left becomes group 0 and is focused. */
  closeGroup(group: GroupIndex): void {
    const other = this.other;
    if (!other) return;
    if (group === this.group) {
      this.openPath = other.openPath;
      this.mode = other.mode;
    }
    this.other = null;
    this.group = 0;
  }
}

/**
 * The workspace as panels inside editor group `group` see it: `openPath`
 * and `mode` are that group's, opening a file there focuses the group
 * first, and a spot to reveal is the focused group's only. Everything else
 * is the workspace itself.
 */
export function groupView(workspace: Workspace, group: GroupIndex): Workspace {
  const focused = () => workspace.group === group;
  const shown = (): GroupShown =>
    focused()
      ? { openPath: workspace.openPath, mode: workspace.mode }
      : (workspace.other ?? { openPath: null, mode: "view" });
  // Derived, so a panel is told only when its own group's value changes, not on every focus change.
  const isFocused = $derived(focused());
  const openPath = $derived(shown().openPath);
  const mode = $derived(shown().mode);
  const reveal = $derived(isFocused ? workspace.reveal : null);
  return new Proxy(workspace, {
    get(target, prop) {
      if (prop === "openPath") return openPath;
      if (prop === "mode") return mode;
      if (prop === "focused") return isFocused;
      if (prop === "reveal") return reveal;
      if (prop === "openFile")
        return (...args: Parameters<Workspace["openFile"]>) => {
          target.focusGroup(group);
          target.openFile(...args);
        };
      const value = Reflect.get(target, prop, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
    set(target, prop, value) {
      if ((prop === "openPath" || prop === "mode") && !focused()) {
        target.other = { ...shown(), [prop]: value };
        return true;
      }
      return Reflect.set(target, prop, value, target);
    },
  });
}
