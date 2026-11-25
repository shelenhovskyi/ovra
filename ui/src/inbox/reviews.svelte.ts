// Review items of the open project, kept current from `review.changed`
// events. The core is the source of truth: nothing here approves, rejects or
// merges on its own; every action is one call made for a person's press.

import type { CoreEvent, ProjectId, ReviewId, ReviewItem } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** What the last approve, reject or undo did, and what it left for the person. */
export type Outcome = {
  id: ReviewId;
  action: "approved" | "rejected" | "undone" | "resolved" | "taken" | "untaken";
  conflicts: string[];
  /** Files a reject restored, or an undo put back. */
  restored: string[];
  /** The commit message an approve committed with. */
  commit?: string | null;
};

export class ReviewInbox {
  /** Items of `project`, newest first, as the core lists them. */
  items = $state<ReviewItem[]>([]);
  project = $state<ProjectId | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Items with an action on the way; their buttons wait for the answer. */
  busy = $state<ReviewId[]>([]);
  outcome = $state<Outcome | null>(null);
  /** Bumped whenever the core says an item changed, so open diffs reload. */
  changes = $state(0);
  private latest = 0;
  private inFlight = false;
  private again = false;
  private readonly off: (() => void)[];

  constructor(private readonly transport: Transport) {
    let wasOpen = false;
    this.off = [
      transport.onEvent((event) => this.apply(event)),
      transport.onStatus?.((status) => {
        if (status !== "open") return;
        // Events may have been missed while disconnected.
        if (wasOpen) void this.reload();
        wasOpen = true;
      }) ?? (() => {}),
    ];
  }

  /** Stops following the core. */
  dispose(): void {
    for (const off of this.off) off();
  }

  get open(): ReviewItem[] {
    return this.items.filter((item) => item.state === "open");
  }

  /** Open items with at least one of the agent's files, and partly approved ones: what waits for the person. */
  get waiting(): ReviewItem[] {
    return this.items.filter(
      (item) =>
        item.state === "partly_approved" || (item.state === "open" && item.files.some((f) => f.by !== "outside")),
    );
  }

  /** Shows `project`'s items; null clears the list. */
  async show(project: ProjectId | null): Promise<void> {
    if (project !== this.project) {
      this.items = [];
      this.outcome = null;
    }
    this.project = project;
    await this.reload();
  }

  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) {
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      const { data } = await this.transport.call({ method: "review.list", params: { project } });
      if (mine !== this.latest) return;
      this.items = data;
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = `Could not list reviews: ${message(err)}`;
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  /** A file's diff, with the item fingerprint it was read with (workers only). */
  async diff(id: ReviewId, path: string): Promise<{ patch: string; fingerprint: string | null }> {
    const { data } = await this.transport.call({ method: "review.diff", params: { id, path } });
    return { patch: data.patch, fingerprint: data.fingerprint ?? null };
  }

  /** Shows every item and open diff as they are now. */
  recheck(): Promise<void> {
    this.changes++;
    return this.reload();
  }

  /**
   * Keeps the changes; with `commit`, the core commits the agent's files with
   * that message. A worker's item goes with the fingerprint of the files shown
   * here, so the core refuses if they changed since the person looked.
   * Protected files land only with `includeProtected`: the person chose so.
   */
  approve(id: ReviewId, commit: string | null, includeProtected = false): Promise<boolean> {
    const fingerprint = this.items.find((i) => i.id === id)?.fingerprint ?? null;
    return this.act(id, async () => {
      const params = {
        id,
        commit,
        ...(fingerprint ? { fingerprint } : {}),
        ...(includeProtected ? { include_protected: true } : {}),
      };
      const { data } = await this.transport.call({ method: "review.approve", params });
      this.settle("approved", data, commit);
    });
  }

  /** Restores the agent's files; files changed outside the agent stay and come back as conflicts. */
  reject(id: ReviewId): Promise<boolean> {
    return this.act(id, async () => {
      const { data } = await this.transport.call({ method: "review.reject", params: { id } });
      this.settle("rejected", data);
    });
  }

  /** Puts back what the last reject of a direct session reverted; files changed since stay and come back as conflicts. */
  undoReject(id: ReviewId): Promise<boolean> {
    return this.act(id, async () => {
      const { data } = await this.transport.call({ method: "review.undo_reject", params: { id } });
      this.settle("undone", data);
    });
  }

  /** Puts the worker's version over the person's for what a partly approved item left. */
  take(id: ReviewId): Promise<boolean> {
    return this.act(id, async () => {
      const { data } = await this.transport.call({ method: "review.take", params: { id } });
      this.settle("taken", data);
    });
  }

  /** Puts back the person's versions from just before the last take. */
  undoTake(id: ReviewId): Promise<boolean> {
    return this.act(id, async () => {
      const { data } = await this.transport.call({ method: "review.undo_take", params: { id } });
      this.settle("untaken", data);
    });
  }

  /** The person is done with the files a partly approved item did not copy. */
  resolve(id: ReviewId): Promise<boolean> {
    return this.act(id, async () => {
      const { data } = await this.transport.call({ method: "review.resolve", params: { id } });
      this.settle("resolved", data);
    });
  }

  /** The same session continues with `text`; a decided item stays decided. */
  comment(id: ReviewId, text: string): Promise<boolean> {
    return this.act(id, async () => {
      await this.transport.call({ method: "review.comment", params: { id, text } });
    });
  }

  private async act(id: ReviewId, run: () => Promise<void>): Promise<boolean> {
    if (this.busy.includes(id)) return false;
    this.busy = [...this.busy, id];
    try {
      await run();
      this.error = null;
      return true;
    } catch (err) {
      this.error = message(err);
      // A refusal often means the item moved on (a worker's files changed
      // since the person looked): show it as it is now, diffs included.
      void this.recheck();
      return false;
    } finally {
      this.busy = this.busy.filter((b) => b !== id);
    }
  }

  /** Re-fetches once, however many changes arrive while a fetch runs. */
  private async refresh(): Promise<void> {
    if (this.inFlight) {
      this.again = true;
      return;
    }
    this.inFlight = true;
    try {
      do {
        this.again = false;
        this.changes++;
        await this.reload();
      } while (this.again);
    } finally {
      this.inFlight = false;
    }
  }

  private settle(
    action: Outcome["action"],
    { item, conflicts, restored }: { item: ReviewItem; conflicts: string[]; restored: string[] },
    commit: string | null = null,
  ): void {
    this.items = this.items.map((i) => (i.id === item.id ? item : i));
    this.outcome = { id: item.id, action, conflicts, restored, commit };
  }

  private apply(event: CoreEvent): void {
    switch (event.type) {
      // The core missed sending us events: everything may be stale.
      case "info.resync":
        void this.refresh();
        return;
      // A file changed (the person's editor, a formatter, git): an open
      // review's files and markers may have changed with it.
      case "review.changed":
      case "fs.changed":
      case "fs.files_changed":
        if (event.data.project === this.project) void this.refresh();
        return;
    }
  }
}

export function message(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}
