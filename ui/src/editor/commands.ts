// Commands owned by the editor that work in any mode.

import { registerCommand } from "../keys/commands";
import type { Workspace } from "../shell/workspace.svelte";

/** Registers `editor.toggle`; returns a function that unregisters it. */
export function registerEditorCommands(workspace: Pick<Workspace, "openPath" | "mode">): () => void {
  return registerCommand({
    id: "editor.toggle",
    title: "Toggle editing the open file",
    keys: "Mod+E",
    run: () => {
      if (workspace.openPath) workspace.mode = workspace.mode === "edit" ? "view" : "edit";
    },
  });
}
