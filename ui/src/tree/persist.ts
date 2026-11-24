// Saves which folders are expanded into the project's state, so the tree
// looks the same when the project is opened again.

import type { ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** The expanded folders saved for `project`; none if the state cannot be read. */
export async function loadExpanded(transport: Transport, project: ProjectId): Promise<string[]> {
  try {
    return (await transport.call({ method: "projects.get_state", params: { id: project } })).data.expanded_dirs;
  } catch {
    return [];
  }
}

/** Writes the expanded folders `delayMs` after the last change; other state fields are left alone. */
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
    try {
      // A partial update: one message, sent synchronously, so it also leaves on page close.
      await this.transport.call({
        method: "projects.set_state",
        params: { id: this.project, state: { expanded_dirs: dirs } },
      });
    } catch {
      // Losing the tree's shape is not worth an error in the UI; the next change retries.
      this.saved = "";
    }
  }
}
