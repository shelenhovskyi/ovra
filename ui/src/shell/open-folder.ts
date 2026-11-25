// "Open a folder…" from anywhere: the system's folder dialog where there is
// one (the tree's Mod+O command), otherwise the start screen's folder field.

import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import type { Workspace } from "./workspace.svelte";

/** Whether "Open a folder…" shows the system's dialog; otherwise it leaves for the start screen. */
export function folderDialogCommand() {
  return listCommands().find((c) => c.id === "tree.open_folder");
}

/** Opens the system's folder dialog, or closes the project and focuses the start screen's folder field. */
export async function openAnotherFolder(workspace: Pick<Workspace, "project" | "openPath">): Promise<void> {
  const dialog = folderDialogCommand();
  if (dialog) {
    await dialog.run();
    return;
  }
  workspace.openPath = null;
  workspace.project = null;
  await tick();
  // The start screen mounts once the project is gone; wait a few frames for it.
  for (let frame = 0; frame < 30; frame++) {
    const field = document.querySelector<HTMLElement>('[data-testid="project-list"] input');
    if (field) {
      field.focus();
      return;
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
}
