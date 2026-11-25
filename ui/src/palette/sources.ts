// What each palette mode lists and what choosing an entry does.

import { modeOf, tabs } from "../editor/open-files.svelte";
import { type Command, listCommands } from "../keys/commands";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { focusOpenFile, mainView } from "../shell/main-view.svelte";
import { folderDialogCommand, openAnotherFolder } from "../shell/open-folder";
import type { Workspace } from "../shell/workspace.svelte";
import { projectFiles } from "./files";
import { type PaletteMode, palette } from "./mode.svelte";
import { idleRows, recentIds } from "./search";

export type Entry = {
  id: string;
  /** Matched and highlighted. */
  label: string;
  /** Shown dimmed after the label, not matched. */
  detail?: string;
  keys?: string;
  /** Shown dimmed before the label: Git, View, Settings. */
  category?: string;
  /** Matched at word starts, after the ranked rows: synonyms and the category. */
  keywords?: string[];
  /** The section it is listed under: Files, Commands, Recent. */
  group?: string;
  run: () => void | Promise<void>;
};

export type Source = {
  name: string;
  placeholder: string;
  empty: string;
  /** Rows shown at most, per group; ranking still covers every entry. */
  limit: number;
  entries: () => Entry[] | Promise<Entry[]>;
  /** What an empty query shows instead of every entry. */
  idle?: (entries: Entry[]) => Entry[];
  /** A leading `>` keeps only the commands. */
  prefix?: boolean;
};

type Target = Pick<Workspace, "project" | "openPath" | "mode" | "openFile" | "paletteOpen">;

/** What an empty "Search or run" suggests while a project is open, when registered. */
const SUGGESTED = ["approvals.focus", "inbox.toggle", "agents.new_session"];

/** The category of a command that names none, by its id's area. */
const AREAS: Record<string, string> = {
  agents: "Agents",
  approvals: "Agents",
  browser: "Agents",
  inbox: "Agents",
  editor: "Editor",
  git: "Git",
  layout: "View",
  palette: "View",
  projects: "Project",
  search: "Search",
  terminal: "Terminal",
  tree: "Files",
};

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

function commands(keep: (command: Command) => boolean = () => true, group?: string): Entry[] {
  return listCommands()
    .filter(keep)
    .map((c) => {
      const category = c.category ?? AREAS[c.id.split(".")[0] ?? ""];
      return {
        id: `command:${c.id}`,
        label: c.title,
        keys: c.keys,
        // "Git: commit" names its category already.
        category: category && !/^[\w ]+: /.test(c.title) ? category : undefined,
        keywords: [...(c.keywords ?? []), ...(category ? [category] : [])],
        group,
        run: c.run,
      };
    });
}

async function files(transport: Transport, workspace: Target, project: Project, group?: string): Promise<Entry[]> {
  return (await projectFiles(transport, project.id)).map((path) => ({
    id: `file:${path}`,
    label: path,
    group,
    // An open file comes back in the mode its tab had; a new one keeps
    // the current mode, so going to a file while editing keeps editing.
    run: () => {
      // A view covering the files makes way, also for the file already open, and the file gets the focus the view had.
      const covered = mainView.current !== null;
      mainView.close(false);
      // Picking the file already shown keeps its preview tab, as Enter in the tree does.
      if (workspace.openPath === path) tabs.current?.makePermanent(path);
      workspace.openFile(path, modeOf(path, workspace.mode));
      if (covered) void focusOpenFile(workspace.mode === "edit");
    },
  }));
}

async function projects(transport: Transport, workspace: Target, group?: string): Promise<Entry[]> {
  return [
    ...(await transport.call({ method: "projects.list" })).data.map((p) => ({
      id: `project:${p.id}`,
      label: p.name,
      detail: p.root,
      group,
      run: () => openProject(transport, workspace, p),
    })),
    // A folder that is not a project yet: the system's dialog, else a path asked
    // for here, so the open project stays until the new one opens.
    { id: "palette.open_folder", label: "Open a folder…", group, run: () => openFolder(workspace) },
  ];
}

export function source(mode: PaletteMode, transport: Transport, workspace: Target): Source {
  switch (mode) {
    case "search": {
      const project = workspace.project;
      const what = project ? "files" : "projects";
      return {
        name: "Search or run",
        placeholder: `Search ${what}, or type > for commands`,
        empty: `No matching ${what} or commands`,
        limit: 10,
        prefix: true,
        entries: async () => [
          ...(project
            ? await files(transport, workspace, project, "Files")
            : await projects(transport, workspace, "Projects")),
          ...commands(undefined, "Commands"),
        ],
        idle: (entries) => idleRows(entries, recentIds(), project ? SUGGESTED.map((id) => `command:${id}`) : null),
      };
    }
    case "commands":
      return {
        name: "Commands",
        placeholder: "Type a command",
        empty: "No matching commands",
        limit: 200,
        entries: () => commands(),
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
        entries: async () => (project ? files(transport, workspace, project) : []),
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
        entries: () => projects(transport, workspace),
      };
  }
}
