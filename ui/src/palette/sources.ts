// What each palette mode lists and what choosing an entry does.

import { modeOf } from "../editor/open-files.svelte";
import { type Command, listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { folderDialogCommand, openAnotherFolder } from "../shell/open-folder";
import type { Workspace } from "../shell/workspace.svelte";
import { projectFiles } from "./files";
import { type PaletteMode, palette } from "./mode.svelte";

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

type Target = Pick<Workspace, "project" | "openPath" | "mode" | "openFile" | "paletteOpen">;

/** Opens `project` in the window, leaving the file to the project's saved state. */
async function openProject(transport: Transport, workspace: Target, project: Project): Promise<void> {
  const opened = (await transport.call({ method: "projects.open", params: { id: project.id } })).data;
  workspace.openPath = null;
  workspace.mode = "view";
  workspace.project = opened;
}

function openFolder(workspace: Target): Promise<void> | void {
  if (folderDialogCommand() || !workspace.project) return openAnotherFolder(workspace);
  palette.mode = "folder";
  workspace.paletteOpen = true;
}

function commands(keep: (command: Command) => boolean = () => true): Entry[] {
  return listCommands()
    .filter(keep)
    .map((c) => ({ id: c.id, label: c.title, keys: c.keys, run: c.run }));
}

export function source(mode: PaletteMode, transport: Transport, workspace: Target): Source {
  switch (mode) {
    case "commands":
      return {
        name: "Commands",
        placeholder: "Type a command",
        empty: "No matching commands",
        limit: 200,
        entries: () => commands(),
      };
    case "settings":
      return {
        name: "Settings",
        placeholder: "Search settings",
        empty: "No matching settings",
        limit: 200,
        entries: () => commands((c) => c.category === "Settings" && c.id !== "palette.settings"),
      };
    case "shortcuts":
      return {
        name: "Keyboard shortcuts",
        placeholder: "Search keyboard shortcuts",
        empty: "No matching shortcuts",
        limit: 200,
        entries: () => commands((c) => c.keys !== undefined),
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
                // An open file comes back in the mode its tab had; a new one keeps
                // the current mode, so going to a file while editing keeps editing.
                run: () => workspace.openFile(path, modeOf(path, workspace.mode)),
              }))
            : [],
      };
    }
    case "folder":
      return {
        name: "Open a folder",
        placeholder: "Folder to open, for example /home/you/my-app",
        empty: workspace.project
          ? `${workspace.project.name} stays open until the new folder opens. Enter opens it.`
          : "Enter opens the folder.",
        limit: 0,
        entries: () => [],
      };
    case "projects":
      return {
        name: "Projects",
        placeholder: "Switch to project",
        empty: "No matching projects",
        limit: 200,
        entries: async () => [
          ...(await transport.call({ method: "projects.list" })).data.map((p) => ({
            id: p.id,
            label: p.name,
            detail: p.root,
            run: () => openProject(transport, workspace, p),
          })),
          // A folder that is not a project yet: the system's dialog, else a path asked
          // for here, so the open project stays until the new one opens.
          { id: "palette.open_folder", label: "Open a folder…", run: () => openFolder(workspace) },
        ],
      };
  }
}
