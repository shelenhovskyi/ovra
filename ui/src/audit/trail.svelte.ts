// The open project's audit log, kept current from `audit.appended`. The log is
// read-only: nothing here edits or removes an entry, and the core offers no
// way to.
//
// It opens on the newest page and reads older pages only when asked
// (`loadOlder`), so a long log never has to be read whole.

import type { AuditEntry, CoreEvent, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** Entries new ones may grow the view to before the oldest are dropped from view, never from the log. */
export const MAX_SHOWN = 2000;
/** What one `audit.list` returns at most (the core's page size). */
export const PAGE = 500;

export class AuditTrail {
  /** Oldest first, by `seq`. */
  entries = $state<AuditEntry[]>([]);
  project = $state<ProjectId | null>(null);
  loading = $state(false);
  loadingOlder = $state(false);
  error = $state<string | null>(null);
  /** The log may hold entries older than the oldest shown. */
  older = $state(false);
  private latest = 0;
  /** Counts project changes, so a page read for another project is dropped. */
  private generation = 0;
  private readonly off: (() => void)[];

  constructor(private readonly transport: Transport) {
    let wasOpen = false;
    this.off = [
      transport.onEvent((event) => this.apply(event)),
      transport.onStatus?.((status) => {
        if (status !== "open") return;
        // Entries may have been appended while disconnected.
        if (wasOpen) void this.reload();
        wasOpen = true;
      }) ?? (() => {}),
    ];
  }

  dispose(): void {
    for (const off of this.off) off();
  }

  /** Shows `project`'s log; null clears it. */
  async show(project: ProjectId | null): Promise<void> {
    if (project !== this.project) {
      this.generation++;
      this.entries = [];
      this.older = false;
      this.loadingOlder = false;
    }
    this.project = project;
    await this.reload();
  }

  /**
   * Reads the newest page, or, when entries are shown, every entry after the
   * newest of them, so nothing between is missed.
   */
  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) {
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      const newest = this.entries[this.entries.length - 1];
      let read: AuditEntry[];
      if (!newest) {
        const page = await this.transport.call({
          method: "audit.list",
          params: { project, order: "newest_first", limit: PAGE },
        });
        if (mine !== this.latest) return;
        read = [...page.data].reverse();
        // Only here: reading newer entries says nothing about older ones, and
        // `loadOlder` may have changed what is known meanwhile.
        this.older = page.data.length === PAGE;
      } else {
        read = [];
        let after = newest.seq;
        for (;;) {
          const page = await this.transport.call({ method: "audit.list", params: { project, after, limit: PAGE } });
          const data: AuditEntry[] = page.data;
          if (mine !== this.latest) return;
          read.push(...data);
          const last: AuditEntry | undefined = data[data.length - 1];
          if (data.length < PAGE || !last) break;
          after = last.seq;
        }
      }
      this.keep(merge(this.entries, read));
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  /** Adds the page of entries just before the oldest shown. */
  async loadOlder(): Promise<void> {
    const project = this.project;
    const oldest = this.entries[0];
    if (!project || !oldest || !this.older || this.loadingOlder) return;
    const generation = this.generation;
    this.loadingOlder = true;
    try {
      const page = await this.transport.call({
        method: "audit.list",
        params: { project, before: oldest.seq, order: "newest_first", limit: PAGE },
      });
      if (generation !== this.generation) return;
      this.older = page.data.length === PAGE;
      // Asked for, so never trimmed away.
      this.entries = merge(page.data, this.entries);
      this.error = null;
    } catch (err) {
      if (generation !== this.generation) return;
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      if (generation === this.generation) this.loadingOlder = false;
    }
  }

  private apply(event: CoreEvent): void {
    if (event.type !== "audit.appended" || event.data.project !== this.project) return;
    this.keep(merge(this.entries, [event.data]));
  }

  /**
   * Shows `entries`, growing to `MAX_SHOWN` (or what older pages made it);
   * past that the oldest leave the view and can be loaded again. Nothing
   * leaves while an older page loads: it joins the oldest shown, and trimming
   * that would leave a gap.
   */
  private keep(entries: AuditEntry[]): void {
    const room = Math.max(MAX_SHOWN, this.entries.length);
    if (entries.length > room && !this.loadingOlder) {
      this.older = true;
      this.entries = entries.slice(entries.length - room);
    } else {
      this.entries = entries;
    }
  }
}

/** Both lists as one, by `seq`, each entry once. */
function merge(a: AuditEntry[], b: AuditEntry[]): AuditEntry[] {
  const bySeq = new Map<number, AuditEntry>();
  for (const entry of [...a, ...b]) bySeq.set(entry.seq, entry);
  return [...bySeq.values()].sort((x, y) => x.seq - y.seq);
}
