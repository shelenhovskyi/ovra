// Test-only fake of the core's `lsp.status` and `lsp.stop`: answers with what a
// test put in, records calls, and sends events when a test asks.

import type { CoreEvent, KnownServer, Request, ResponseFor, ServerInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export const server = (id: string, extra: Partial<ServerInfo> = {}): ServerInfo =>
  ({
    id,
    project: "p1",
    language: "rust",
    name: "rust-analyzer",
    sandbox: "bubblewrap",
    state: "running",
    ...extra,
  }) as ServerInfo;

export class FakeLspCore implements Transport {
  calls: Request[] = [];
  trusted = true;
  sandbox: string | null = "bubblewrap";
  servers: ServerInfo[] = [];
  known: KnownServer[] = [];
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    switch (request.method) {
      case "lsp.status":
        return {
          type: "lsp.status",
          data: { trusted: this.trusted, sandbox: this.sandbox, servers: [...this.servers], known: [...this.known] },
        } as ResponseFor<R>;
      case "lsp.stop":
        return { type: "ok" } as ResponseFor<R>;
      default:
        throw new Error(`unexpected ${request.method}`);
    }
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  changed(info: ServerInfo): void {
    for (const listener of this.listeners) listener({ type: "lsp.changed", data: { server: info } });
  }

  resync(): void {
    for (const listener of this.listeners) listener({ type: "info.resync", data: { skipped: 1 } });
  }
}
