// Commands owned by the palette: open it in each mode, and leave the project.
// Registering them also starts noting the files opened, for Recent files.
// Keyboard shortcuts is a palette list; Settings is a view of its own (ui/src/settings).

import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import { registerContextKeys } from "../keys/context-keys";
import type { Workspace } from "../shell/workspace.svelte";
import { type PaletteMode, palette } from "./mode.svelte";
import { trackRecentFiles } from "./recent-files.svelte";

type Target = Pick<Workspace, "paletteOpen" | "project" | "openPath">;

/** Registers the palette commands; returns a function that unregisters them. */
export function registerPaletteCommands(workspace: Target): () => void {
  const show = (mode: PaletteMode) => () => {
    palette.mode = mode;
    workspace.paletteOpen = true;
  };
  const goToFile = () => show(workspace.project ? "files" : "projects")();
  const unregister = [
    registerCommand({ id: "palette.show", title: "Show all commands", keys: "Mod+Shift+P", run: show("commands") }),
    registerCommand({ id: "palette.go_to_file", title: "Go to file", keys: "Mod+P", category: "File", run: goToFile }),
    // Mod+E, JetBrains' key for it, toggles editing here; no key of its own.
    registerCommand({
      id: "palette.recent_files",
      title: "Recent files",
      category: "File",
      keywords: ["recently opened"],
      run: () => show(workspace.project ? "recent" : "projects")(),
    }),
    // The title bar's search field: files (projects without one open) and commands.
    registerCommand({
      id: "palette.search",
      title: "Search or run",
      keys: "Mod+K",
      keywords: ["quick open"],
      run: show("search"),
    }),
    registerCommand({
      id: "palette.switch_project",
      title: "Switch project",
      category: "Project",
      run: show("projects"),
    }),
    registerCommand({
      id: "palette.shortcuts",
      title: "Keyboard shortcuts",
      category: "Help",
      keywords: ["keybindings", "hotkeys"],
      run: show("shortcuts"),
    }),
    registerCommand({
      id: "palette.close_project",
      title: "Close project",
      category: "Project",
      run: async () => {
        workspace.openPath = null;
        workspace.project = null;
        // The focused tree row is gone: focus the start screen's first project once the list has loaded.
        await tick();
        await focusStartScreen();
      },
    }),
    registerContextKeys(PALETTE_KEYS),
    trackRecentFiles(workspace),
  ];
  return () => {
    for (const off of unregister) off();
  };
}

/** What the palette's own keys do, for Settings > Keyboard. */
const PALETTE_KEYS = [
  { keys: "Up/Down", title: "Previous or next result" },
  { keys: "Ctrl+N", title: "Next result (also Ctrl+J)" },
  { keys: "Ctrl+P", title: "Previous result (also Ctrl+K)" },
  { keys: "PageUp/PageDown", title: "Move by a page" },
  { keys: "Enter", title: "Run or open the result" },
  { keys: "Escape", title: "Close" },
].map((key) => ({ ...key, where: "Search or run" }));

/** Focuses the first project on the start screen, or its folder field when there are none; waits a few frames for the list. */
export async function focusStartScreen(): Promise<void> {
  for (let frame = 0; frame < 30; frame++) {
    // The person opened the palette or another dialog while the list loaded: focus is theirs now.
    if (document.querySelector('[role="dialog"][aria-modal="true"]')) return;
    const screen = document.querySelector('[data-testid="project-list"]');
    const target =
      screen?.querySelector<HTMLElement>("ul button") ??
      (screen?.textContent?.includes("No projects yet") ? screen.querySelector<HTMLElement>("input") : null);
    if (target) {
      target.focus();
      return;
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
}
