// One server's output as the panel shows it: the kept lines from
// `servers.logs`, then live `servers.output` lines. Line numbers go on across
// restarts, so a line is new when its number is higher than the last one.
// Only the newest `MAX_LINES` stay in memory.

import type { CoreEvent, LogLine, LogPage, DevServerId as ServerId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { message } from "./servers.svelte";

export const MAX_LINES = 5000;
const PAGE = 1000;

export class ServerLog {
  lines = $state.raw<LogLine[]>([]);
  /** Whether older lines were dropped to stay under `MAX_LINES`. */
  dropped = $state(false);
  loading = $state(true);
  error = $state<string | null>(null);
  private last = 0;
  /** Live lines that arrive while the kept ones are read. */
  private pending: LogLine[] | null = [];
  private readonly off: () => void;

  constructor(
    private readonly transport: Transport,
    readonly id: ServerId,
  ) {
    this.off = transport.onEvent((event) => this.apply(event));
  }

  dispose(): void {
    this.off();
  }

  async load(): Promise<void> {
    let kept: LogLine[] = [];
    let since: number | null = null;
    try {
      for (;;) {
        const { data }: { data: LogPage } = await this.transport.call({
          method: "servers.logs",
          params: { id: this.id, since, limit: PAGE },
        });
        kept = [...kept, ...data.lines].slice(-MAX_LINES);
        if (data.lines.length < PAGE || data.next === since) break;
        since = data.next;
      }
      this.error = null;
    } catch (err) {
      this.error = message(err);
    }
    const live = this.pending ?? [];
    this.pending = null;
    this.loading = false;
    this.append([...kept, ...live]);
  }

  private apply(event: CoreEvent): void {
    if (event.type !== "servers.output" || event.data.id !== this.id) return;
    if (this.pending) this.pending.push(...event.data.lines);
    else this.append(event.data.lines);
  }

  private append(lines: LogLine[]): void {
    const fresh = lines.filter((line) => {
      if (line.n <= this.last) return false;
      this.last = line.n;
      return true;
    });
    if (fresh.length === 0) return;
    const all = [...this.lines, ...fresh];
    if (all.length > MAX_LINES) this.dropped = true;
    this.lines = all.slice(-MAX_LINES);
  }
}
