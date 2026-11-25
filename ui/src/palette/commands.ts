// Commands owned by the palette: open it in each mode, and leave the project.
// Settings and keyboard shortcuts are palette lists until they get views of their own.

import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import type { Workspace } from "../shell/workspace.svelte";
import { type PaletteMode, palette } from "./mode.svelte";

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
      id: "palette.settings",
      title: "Open settings",
      keys: "Mod+,",
      category: "Settings",
      keywords: ["preferences", "options"],
      run: show("settings"),
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
  ];
  return () => {
    for (const off of unregister) off();
  };
}

/** Focuses the first project on the start screen, or its folder field when there are none; waits a few frames for the list. */
async function focusStartScreen(): Promise<void> {
  for (let frame = 0; frame < 30; frame++) {
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
