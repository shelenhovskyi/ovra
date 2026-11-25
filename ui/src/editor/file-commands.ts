// Commands on the open file that do not need the editor: copy its path, open it in another app.

import { registerCommand } from "../keys/commands";
import { CoreError, type Transport } from "../lib/transport";
import type { Workspace } from "../shell/workspace.svelte";
import { absolutePath } from "./context-menu";
import { tabs } from "./open-files.svelte";

type Target = Pick<Workspace, "project" | "openPath">;

/** Registers the file commands; returns a function that unregisters them. */
export function registerFileCommands(workspace: Target, transport: Transport): () => void {
  const copyPath = async () => {
    const root = workspace.project?.root;
    const path = workspace.openPath;
    if (root === undefined || !path) return;
    await navigator.clipboard.writeText(absolutePath(root, path));
  };
  const openExternal = async () => {
    const project = workspace.project?.id;
    const path = workspace.openPath;
    if (!project || !path) return;
    try {
      await transport.call({ method: "open.external", params: { project, path, line: null } });
    } catch (err) {
      // The core says why, for example that the file could run as a program.
      const why = err instanceof CoreError ? `: ${err.error.message}` : "";
      tabs.notice = `Could not open ${path} in another app${why}.`;
    }
  };
  const unregister = [
    registerCommand({ id: "file.copy_path", title: "Copy path", category: "File", run: copyPath }),
    registerCommand({
      id: "file.open_external",
      title: "Open in external app",
      category: "File",
      keywords: ["default app", "open with"],
      run: openExternal,
    }),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
