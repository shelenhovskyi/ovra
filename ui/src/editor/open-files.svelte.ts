// Remembers which files were open in a project and reopens the active one.
// Writes are partial: only `open_files` and `active_file`, so fields of other
// panels (the tree's expanded folders) stay as they are.

import type { ProjectId, ProjectStatePatch } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Workspace } from "../shell/workspace.svelte";

/** Oldest entries are dropped beyond this many open files. */
const MAX_OPEN_FILES = 20;

export class OpenFiles {
  private files: string[] = [];
  private active: string | null = null;
  private writes = Promise.resolve();

  constructor(
    private readonly transport: Transport,
    private readonly project: ProjectId,
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

  /** Records `path` (or nothing) as the active file; writes run in order. */
  opened(path: string | null): Promise<void> {
    if (path === this.active) return this.writes;
    this.active = path;
    if (path) this.files = [...this.files.filter((p) => p !== path), path].slice(-MAX_OPEN_FILES);
    const files = this.files;
    this.writes = this.writes
      .then(async () => {
        const state: ProjectStatePatch = { open_files: files, active_file: path };
        await this.transport.call({ method: "projects.set_state", params: { id: this.project, state } });
      })
      .catch(() => {
        // Losing the list of open files is harmless; the next change writes it again.
      });
    return this.writes;
  }
}

/** Keeps the open project's files in its state; returns a function that stops. */
export function persistOpenFiles(transport: Transport, workspace: Workspace): () => void {
  return $effect.root(() => {
    let current: { project: ProjectId; files: OpenFiles; ready: boolean } | null = null;

    $effect(() => {
      const project = workspace.project?.id;
      if (!project) return;
      const tracked = { project, files: new OpenFiles(transport, project), ready: false };
      current = tracked;
      tracked.files.restore().then(
        (active) => {
          if (current !== tracked) return;
          if (active && workspace.openPath === null) workspace.openFile(active);
          tracked.ready = true;
          void tracked.files.opened(workspace.openPath);
        },
        () => {
          tracked.ready = true;
        },
      );
      return () => {
        if (current === tracked) current = null;
      };
    });

    $effect(() => {
      const path = workspace.openPath;
      if (current?.ready) void current.files.opened(path);
    });
  });
}
