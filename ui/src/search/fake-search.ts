// Test-only fake of the core's search requests: records calls, answers
// search.start with counting ids and lets a test push events.

import type { CoreEvent, FileMatches, Request, ResponseFor, SearchId } from "../lib/protocol";
import type { CoreError, Transport } from "../lib/transport";

export const file = (path: string, ...lines: [number, string, [number, number][]][]): FileMatches => ({
  path,
  lines: lines.map(([line, text, ranges]) => ({ line, text, column: 0, ranges })),
});

export class FakeSearchCore implements Transport {
  calls: Request[] = [];
  /** Makes the next search.start fail with this error. */
  failNext: CoreError | null = null;
  /** Events the core sends before it answers search.start. */
  beforeReply: (id: SearchId) => CoreEvent[] = () => [];
  /** Files for fs.read_file and fs.write_file. */
  disk = new Map<string, string>();
  private last = 0;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    if (request.method === "search.start") {
      if (this.failNext) {
        const failure = this.failNext;
        this.failNext = null;
        throw failure;
      }
      const id = ++this.last;
      for (const event of this.beforeReply(id)) this.emit(event);
      return { type: "search.started", data: { id } } as ResponseFor<R>;
    }
    if (request.method === "fs.read_file") {
      const text = this.disk.get(request.params.path) ?? "";
      const data = { path: request.params.path, size: text.length, mtime_ms: 1, body: { kind: "text", text } };
      return { type: "fs.file", data } as ResponseFor<R>;
    }
    if (request.method === "fs.write_file") {
      this.disk.set(request.params.path, request.params.text);
      return { type: "fs.written", data: { mtime_ms: 2 } } as ResponseFor<R>;
    }
    return { type: "ok" } as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  results(id: SearchId, ...files: FileMatches[]): void {
    this.emit({ type: "search.results", data: { id, files } });
  }

  done(id: SearchId, extra: { truncated?: boolean; cancelled?: boolean } = {}): void {
    this.emit({
      type: "search.done",
      data: { id, files: 0, lines: 0, truncated: extra.truncated ?? false, cancelled: extra.cancelled ?? false },
    });
  }

  /** The methods called, in order. */
  methods(): string[] {
    return this.calls.map((call) => call.method);
  }
}
