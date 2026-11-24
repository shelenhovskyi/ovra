// The open files of a project, shown as tabs and kept in the project state
// so the active file comes back when the project is reopened. Writes are
// partial (`open_files` and `active_file` only), so fields of other panels,
// like the tree's expanded folders, stay as they are.

import { untrack } from "svelte";
import type { ProjectId, ProjectStatePatch } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Workspace } from "../shell/workspace.svelte";

/** Beyond this many tabs, the leftmost inactive one is closed. */
const MAX_OPEN_FILES = 20;

export class OpenFiles {
  /** Tab order. */
  files = $state<string[]>([]);
  active = $state<string | null>(null);
  private writes = Promise.resolve();

  constructor(
    private readonly transport: Transport,
    readonly project: ProjectId,
  ) {}

  /** Loads the stored state; resolves to the file that was active. */
  async restore(): Promise<string | null> {
    const { data: state } = await this.transport.call({
      method: "projects.get_state",
      params: { id: this.project },
    });
    this.files = state.open_files;
    this.active = state.active_file;
    return state.active_file;
  }

  /** Makes `path` (or nothing) the active file, adding a tab at the end if needed. */
  opened(path: string | null): Promise<void> {
    if (path === this.active) return this.writes;
    this.active = path;
    if (path && !this.files.includes(path)) {
      const files = [...this.files, path];
      while (files.length > MAX_OPEN_FILES) files.splice(files[0] === path ? 1 : 0, 1);
      this.files = files;
    }
    return this.write();
  }

  /** Removes a tab; resolves to the file to show next if it was the active one. */
  close(path: string): string | null {
    const index = this.files.indexOf(path);
    if (index === -1) return this.active;
    this.files = this.files.filter((p) => p !== path);
    if (this.active === path) this.active = this.files[index] ?? this.files[index - 1] ?? null;
    void this.write();
    return this.active;
  }

  /** The tab `offset` places away from the active one, wrapping around. */
  neighbour(offset: number): string | null {
    if (this.files.length === 0) return null;
    const index = this.active ? this.files.indexOf(this.active) : -1;
    const count = this.files.length;
    return this.files[(((index + offset) % count) + count) % count] ?? null;
  }

  private write(): Promise<void> {
    const state: ProjectStatePatch = { open_files: this.files, active_file: this.active };
    this.writes = this.writes
      .then(async () => {
        await this.transport.call({ method: "projects.set_state", params: { id: this.project, state } });
      })
      .catch(() => {
        // Losing the list of open files is harmless; the next change writes it again.
      });
    return this.writes;
  }
}

/** The open project's tabs, or null before its state is loaded. */
export const tabs = $state<{ current: OpenFiles | null }>({ current: null });

/** Shows the file at `path` (or nothing), keeping the current mode. */
export function show(workspace: Pick<Workspace, "openPath" | "mode" | "openFile">, path: string | null): void {
  if (path) workspace.openFile(path, workspace.mode);
  else workspace.openPath = null;
}

/** Keeps `tabs.current` in step with the open project; returns a function that stops. */
export function persistOpenFiles(transport: Transport, workspace: Workspace): () => void {
  return $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      tabs.current = null;
      if (!project) return;
      const files = new OpenFiles(transport, project);
      let live = true;
      const ready = () => {
        if (!live) return;
        tabs.current = files;
        void files.opened(workspace.openPath);
      };
      files.restore().then((active) => {
        if (live && active && workspace.openPath === null) workspace.openFile(active);
        ready();
      }, ready);
      return () => {
        live = false;
      };
    });

    $effect(() => {
      const path = workspace.openPath;
      const current = tabs.current;
      untrack(() => current?.opened(path));
    });
  });
}
