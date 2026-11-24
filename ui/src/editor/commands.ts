// Commands owned by the editor that work in any mode.

import { registerCommand } from "../keys/commands";
import type { Workspace } from "../shell/workspace.svelte";
import { focusMain, requestClose, show, tabs } from "./open-files.svelte";

type Target = Pick<Workspace, "openPath" | "mode" | "openFile">;

/** Switches between viewing and editing; files that cannot be edited say why instead. */
export async function toggle(workspace: Target): Promise<void> {
  const path = workspace.openPath;
  if (!path) return;
  if (workspace.mode === "edit") {
    workspace.mode = "view";
    await focusMain();
    return;
  }
  tabs.notice = null;
  const buffers = tabs.current?.buffers;
  const buffer = buffers?.open(path);
  await buffer?.loaded;
  if (workspace.openPath !== path) return;
  if (buffers && buffer && buffer.status !== "ready") {
    buffers.discard(path);
    tabs.notice =
      buffer.status === "binary"
        ? `${path} is a binary file and cannot be edited here.`
        : buffer.status === "too_large"
          ? `${path} is too large to edit here.`
          : `${path} cannot be edited: ${buffer.error ?? "it could not be read"}.`;
    return;
  }
  workspace.mode = "edit";
}

/** Saves the open file if it has unsaved text (loaded or as a draft); otherwise does nothing. */
async function saveActive(workspace: Target): Promise<void> {
  const path = workspace.openPath;
  const buffers = tabs.current?.buffers;
  if (!path || !buffers?.isDirty(path)) return;
  await buffers.open(path).save();
}

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
      run: () => toggle(workspace),
    }),
    // Registered in every mode, so Mod+S never reaches the browser's "Save page".
    registerCommand({ id: "editor.save", title: "Save file", keys: "Mod+S", run: () => saveActive(workspace) }),
    registerCommand({ id: "editor.next_tab", title: "Next tab", keys: "Ctrl+Tab", run: step(1) }),
    registerCommand({ id: "editor.previous_tab", title: "Previous tab", keys: "Ctrl+Shift+Tab", run: step(-1) }),
    registerCommand({
      id: "editor.close_tab",
      title: "Close tab",
      keys: "Mod+W",
      run: () => {
        const active = tabs.current?.active;
        if (active) requestClose(workspace, active);
      },
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
