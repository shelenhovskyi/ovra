// Git data for one open file: its text in the index (for change markers) and
// its blame, each fetched lazily and kept until the file or the repository
// changes. A core without git (`unavailable`) is not asked again; other
// failures (no repository, untracked file) are retried only after the next
// change, never in a loop.

import type { Blame, Hunk, ProjectId } from "../../lib/protocol";
import { CoreError, type Transport } from "../../lib/transport";
import { type LineHunk, splitLines } from "./linediff";

type Options = { transport: Transport; project: ProjectId; path: string };

export class GitFile {
  private index: Promise<string[] | null> | null = null;
  private blamed: Promise<Blame | null> | null = null;
  private unavailable = false;
  private readonly listeners = new Set<() => void>();

  constructor(private readonly options: Options) {}

  /** The file's lines in the index; null when there is nothing to compare with. */
  indexLines(): Promise<string[] | null> {
    this.index ??= this.ask(async () => {
      const { data } = await this.options.transport.call({
        method: "git.show",
        params: { project: this.options.project, path: this.options.path, rev: "index" },
      });
      return data.exists && data.text !== null ? splitLines(data.text) : null;
    });
    return this.index;
  }

  /** Who last changed each line of the file on disk, or null. */
  blame(): Promise<Blame | null> {
    this.blamed ??= this.ask(async () => {
      const { data } = await this.options.transport.call({
        method: "git.blame",
        params: { project: this.options.project, path: this.options.path },
      });
      return data;
    });
    return this.blamed;
  }

  /** The repository (`fs.git_changed`) or the file on disk changed: drops what was fetched. */
  changed(): void {
    this.index = null;
    this.blamed = null;
    for (const listener of this.listeners) listener();
  }

  /** The file now lives at `path`: asks again under the new name. */
  moveTo(path: string): void {
    this.options.path = path;
    this.changed();
  }

  /** Calls `listener` whenever cached data was dropped; returns a function that stops. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /**
   * Stages the part of the unstaged diff that holds `hunk` (lines of the saved
   * file). Resolves to an error message for the person, or null when staged.
   */
  async stage(hunk: LineHunk): Promise<string | null> {
    const params = { project: this.options.project, path: this.options.path };
    try {
      const { data } = await this.options.transport.call({ method: "git.diff", params: { ...params, staged: false } });
      if (data.truncated || data.binary) return "This diff is too large to stage by hunk.";
      const picked = pickHunk(data.hunks, hunk);
      if (!picked) return "This change is no longer in the unstaged diff.";
      await this.options.transport.call({ method: "git.stage_hunk", params: { ...params, hunk: picked } });
      this.changed();
      return null;
    } catch (err) {
      if (err instanceof CoreError && err.code === "conflict") {
        return "The file changed since its diff was read. Try again.";
      }
      return `Could not stage the change: ${err instanceof Error ? err.message : String(err)}`;
    }
  }

  private async ask<T>(fetch: () => Promise<T | null>): Promise<T | null> {
    if (this.unavailable) return null;
    try {
      return await fetch();
    } catch (err) {
      if (err instanceof CoreError && err.code === "unavailable") this.unavailable = true;
      return null;
    }
  }
}

/**
 * The git hunk (with its context lines) whose new side overlaps `hunk`, a
 * 0-based line hunk. A deletion overlaps the lines on either side of it.
 */
export function pickHunk(hunks: readonly Hunk[], hunk: LineHunk): Hunk | undefined {
  const from = hunk.newCount > 0 ? hunk.newStart + 1 : Math.max(hunk.newStart, 1);
  const to = hunk.newCount > 0 ? hunk.newStart + hunk.newCount : hunk.newStart + 1;
  return hunks.find((h) => h.new_start <= to && from <= h.new_start + Math.max(h.new_lines, 1) - 1);
}
