// What each palette mode lists and what choosing an entry does.

import { listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Workspace } from "../shell/workspace.svelte";
import { projectFiles } from "./files";
import type { PaletteMode } from "./mode.svelte";

export type Entry = {
  id: string;
  /** Matched and highlighted. */
  label: string;
  /** Shown dimmed after the label, not matched. */
  detail?: string;
  keys?: string;
  run: () => void | Promise<void>;
};

export type Source = {
  name: string;
  placeholder: string;
  empty: string;
  /** Rows shown at most; ranking still covers every entry. */
  limit: number;
  entries: () => Entry[] | Promise<Entry[]>;
};

type Target = Pick<Workspace, "project" | "openPath" | "mode" | "openFile">;

/** Opens `project` in the window, leaving the file to the project's saved state. */
async function openProject(transport: Transport, workspace: Target, project: Project): Promise<void> {
  const opened = (await transport.call({ method: "projects.open", params: { id: project.id } })).data;
  workspace.openPath = null;
  workspace.mode = "view";
  workspace.project = opened;
}

export function source(mode: PaletteMode, transport: Transport, workspace: Target): Source {
  switch (mode) {
    case "commands":
      return {
        name: "Commands",
        placeholder: "Type a command",
        empty: "No matching commands",
        limit: 200,
        entries: () => listCommands().map((c) => ({ id: c.id, label: c.title, keys: c.keys, run: c.run })),
      };
    case "files": {
      const project = workspace.project;
      return {
        name: "Files",
        placeholder: project ? `Go to file in ${project.name}` : "Open a project first",
        empty: "No matching files",
        limit: 50,
        entries: async () =>
          project
            ? (await projectFiles(transport, project.id)).map((path) => ({
                id: path,
                label: path,
                // Keep the mode: going to a file while editing keeps editing.
                run: () => workspace.openFile(path, workspace.mode),
              }))
            : [],
      };
    }
    case "projects":
      return {
        name: "Projects",
        placeholder: "Switch to project",
        empty: "No matching projects",
        limit: 200,
        entries: async () =>
          (await transport.call({ method: "projects.list" })).data.map((p) => ({
            id: p.id,
            label: p.name,
            detail: p.root,
            run: () => openProject(transport, workspace, p),
          })),
      };
  }
}
