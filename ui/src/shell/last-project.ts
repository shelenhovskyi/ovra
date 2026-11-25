// The project open in the window, kept in the page's storage so a reload (or
// a restart of the window) opens it again. Closing the project forgets it.

import type { ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import type { Workspace } from "./workspace.svelte";

const KEY = "ovra.project";

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Storage can be switched off; the window then just starts on the start screen.
    return null;
  }
}

/** Remembers `project` as the one to open next time, or forgets it for null. */
export function rememberProject(project: ProjectId | null, store = storage()): void {
  try {
    if (project) store?.setItem(KEY, project);
    else store?.removeItem(KEY);
  } catch {
    // A full or blocked storage only costs the restore.
  }
}

/**
 * Opens the remembered project unless one was opened meanwhile. A project
 * the core no longer knows is forgotten; a lost connection keeps it for next time.
 */
export async function restoreProject(
  transport: Transport,
  workspace: Pick<Workspace, "project" | "openPath" | "mode">,
  store = storage(),
): Promise<void> {
  let id: string | null = null;
  try {
    id = store?.getItem(KEY) ?? null;
  } catch {
    return;
  }
  if (!id) return;
  try {
    const project = (await transport.call({ method: "projects.open", params: { id } })).data;
    if (workspace.project) return;
    workspace.openPath = null;
    workspace.mode = "view";
    workspace.project = project;
  } catch (err) {
    if (err instanceof CoreError) rememberProject(null, store);
  }
}
