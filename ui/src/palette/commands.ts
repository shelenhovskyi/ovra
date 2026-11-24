// Commands owned by the palette.

import { registerCommand } from "../keys/commands";
import type { Workspace } from "../shell/workspace.svelte";

/** Registers `palette.show`; returns a function that unregisters it. */
export function registerPaletteCommands(workspace: Pick<Workspace, "paletteOpen">): () => void {
  return registerCommand({
    id: "palette.show",
    title: "Show command palette",
    keys: "Mod+Shift+P",
    run: () => {
      workspace.paletteOpen = true;
    },
  });
}
