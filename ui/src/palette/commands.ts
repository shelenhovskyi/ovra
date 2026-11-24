// Commands owned by the palette: open it in each mode, and leave the project.

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
  const unregister = [
    registerCommand({ id: "palette.show", title: "Show all commands", keys: "Mod+Shift+P", run: show("commands") }),
    registerCommand({
      id: "palette.go_to_file",
      title: "Go to file",
      keys: "Mod+P",
      run: () => show(workspace.project ? "files" : "projects")(),
    }),
    registerCommand({ id: "palette.switch_project", title: "Switch project", run: show("projects") }),
    registerCommand({
      id: "palette.close_project",
      title: "Close project",
      run: () => {
        workspace.openPath = null;
        workspace.project = null;
      },
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
