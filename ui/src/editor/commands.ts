// Commands owned by the editor that work in any mode.

import { registerCommand } from "../keys/commands";
import type { Workspace } from "../shell/workspace.svelte";
import { show, tabs } from "./open-files.svelte";

type Target = Pick<Workspace, "openPath" | "mode" | "openFile">;

/** Registers the editor and tab commands; returns a function that unregisters them. */
export function registerEditorCommands(workspace: Target): () => void {
  const step = (offset: number) => () => {
    const next = tabs.current?.neighbour(offset);
    if (next) show(workspace, next);
  };
  const unregister = [
    registerCommand({
      id: "editor.toggle",
      title: "Toggle editing the open file",
      keys: "Mod+E",
      run: () => {
        if (workspace.openPath) workspace.mode = workspace.mode === "edit" ? "view" : "edit";
      },
    }),
    registerCommand({ id: "editor.next_tab", title: "Next tab", keys: "Ctrl+Tab", run: step(1) }),
    registerCommand({ id: "editor.previous_tab", title: "Previous tab", keys: "Ctrl+Shift+Tab", run: step(-1) }),
    registerCommand({
      id: "editor.close_tab",
      title: "Close tab",
      keys: "Mod+W",
      run: () => {
        const current = tabs.current;
        if (current?.active) show(workspace, current.close(current.active));
      },
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
