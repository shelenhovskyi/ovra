// Auto save (Settings > Editor): "after_delay" saves a buffer a moment after
// its last edit; "on_focus_change" saves when the window loses focus or
// another tab (or mode) takes the editor. Saving goes through Buffer.save,
// so a file changed on disk is never overwritten: that is a conflict, shown
// as before, and auto save leaves the buffer alone until it is resolved.
// A buffer whose save failed waits for the person to save it themselves.

import { type AutoSave, editorPrefs } from "./prefs.svelte";

/** How long after the last edit "after_delay" saves. */
export const AUTO_SAVE_DELAY_MS = 1000;

/** What auto save needs of a buffer (Buffer in buffers.svelte.ts). */
export type Saveable = {
  status: string;
  dirty: boolean;
  conflict: boolean;
  deleted: boolean;
  error: string | null;
  save(): Promise<boolean>;
};

export class AutoSaver {
  private readonly timers = new Map<Saveable, ReturnType<typeof setTimeout>>();
  private readonly saving = new Set<Saveable>();

  constructor(private readonly mode: () => AutoSave) {}

  /** Whether auto save may write `buffer` now. */
  static ready(buffer: Saveable): boolean {
    return buffer.status === "ready" && buffer.dirty && !buffer.conflict && !buffer.deleted && !buffer.error;
  }

  /** The text of `buffer` changed: in "after_delay", saving waits for the edits to stop. */
  edited(buffer: Saveable): void {
    this.cancel(buffer);
    if (this.mode() !== "after_delay" || !buffer.dirty) return;
    this.timers.set(
      buffer,
      setTimeout(() => {
        this.timers.delete(buffer);
        if (this.mode() === "after_delay") void this.save(buffer);
      }, AUTO_SAVE_DELAY_MS),
    );
  }

  /** Focus left these buffers (the window blurred, or the tab changed): "on_focus_change" saves them. */
  left(buffers: Iterable<Saveable>): void {
    if (this.mode() !== "on_focus_change") return;
    for (const buffer of buffers) void this.save(buffer);
  }

  /** Drops a pending save, as when the buffer is discarded. */
  cancel(buffer: Saveable): void {
    const timer = this.timers.get(buffer);
    if (timer !== undefined) clearTimeout(timer);
    this.timers.delete(buffer);
  }

  private async save(buffer: Saveable): Promise<void> {
    if (this.saving.has(buffer) || !AutoSaver.ready(buffer)) return;
    this.saving.add(buffer);
    try {
      await buffer.save();
    } finally {
      this.saving.delete(buffer);
    }
  }
}

/** The editor's auto save, following `editorPrefs.autoSave`. */
export const autoSaver = new AutoSaver(() => editorPrefs.autoSave);
