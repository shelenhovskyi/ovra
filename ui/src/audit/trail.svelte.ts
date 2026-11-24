// The open project's audit log, kept current from `audit.appended`. The log is
// read-only: nothing here edits or removes an entry, and the core offers no
// way to.

import type { AuditEntry, CoreEvent, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";

/** Entries kept in memory; older ones are dropped from view, never from the log. */
export const MAX_SHOWN = 2000;
/** What one `audit.list` returns at most (the core's page size). */
const PAGE = 500;

export class AuditTrail {
  /** Oldest first, by `seq`, at most `MAX_SHOWN`. */
  entries = $state<AuditEntry[]>([]);
  project = $state<ProjectId | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Older entries exist that are not shown. */
  cut = $state(false);
  private latest = 0;
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
      this.entries = [];
      this.cut = false;
    }
    this.project = project;
    await this.reload();
  }

  /** Reads the whole log of the project again, page by page. */
  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) {
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      const all: AuditEntry[] = [];
      let trimmed = false;
      let after: number | null = null;
      for (;;) {
        const page = await this.transport.call({ method: "audit.list", params: { project, after, limit: PAGE } });
        const data: AuditEntry[] = page.data;
        if (mine !== this.latest) return;
        all.push(...data);
        // Only the newest are kept while paging through a long log.
        if (all.length > MAX_SHOWN) {
          all.splice(0, all.length - MAX_SHOWN);
          trimmed = true;
        }
        const last: AuditEntry | undefined = data[data.length - 1];
        if (data.length < PAGE || !last) break;
        after = last.seq;
      }
      // Entries shown live while the pages loaded are newer or already in `all`.
      this.cut = trimmed;
      this.keep(merge(all, this.entries));
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  private apply(event: CoreEvent): void {
    if (event.type !== "audit.appended" || event.data.project !== this.project) return;
    this.keep(merge(this.entries, [event.data]));
  }

  /** Shows the newest `MAX_SHOWN` of `entries`; dropping any marks the view as cut. */
  private keep(entries: AuditEntry[]): void {
    if (entries.length > MAX_SHOWN) {
      this.cut = true;
      this.entries = entries.slice(entries.length - MAX_SHOWN);
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
