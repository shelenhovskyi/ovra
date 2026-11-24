// Saves which folders are expanded into the project's state, so the tree
// looks the same when the project is opened again.

import type { ProjectId, ProjectState } from "../lib/protocol";
import type { Transport } from "../lib/transport";

const EMPTY: ProjectState = { open_files: [], active_file: null, expanded_dirs: [] };

/** The expanded folders saved for `project`; none if the state cannot be read. */
export async function loadExpanded(transport: Transport, project: ProjectId): Promise<string[]> {
  try {
    return (await transport.call({ method: "projects.get_state", params: { id: project } })).data.expanded_dirs;
  } catch {
    return [];
  }
}

/** Writes the expanded folders `delayMs` after the last change, keeping the rest of the state. */
export class ExpandedSaver {
  private saved: string;
  private pending: string[] | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly transport: Transport,
    private readonly project: ProjectId,
    initial: string[],
    private readonly delayMs = 300,
  ) {
    this.saved = initial.join("\n");
  }

  update(dirs: string[]): void {
    if (this.pending === null && dirs.join("\n") === this.saved) return;
    this.pending = dirs;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.flush(), this.delayMs);
  }

  /** Saves a pending change now (also called when the tree closes). */
  async flush(): Promise<void> {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const dirs = this.pending;
    this.pending = null;
    if (!dirs || dirs.join("\n") === this.saved) return;
    this.saved = dirs.join("\n");
    const id = this.project;
    try {
      // set_state replaces the whole state, so keep what other panels saved.
      const state = await this.transport.call({ method: "projects.get_state", params: { id } }).then(
        (r) => r.data,
        () => EMPTY,
      );
      await this.transport.call({
        method: "projects.set_state",
        params: { id, state: { ...state, expanded_dirs: dirs } },
      });
    } catch {
      // Losing the tree's shape is not worth an error in the UI; the next change retries.
      this.saved = "";
    }
  }
}
