// Test-only fake of the core's `audit.list`: pages through `entries` like the
// core and lets a test push `audit.appended`.

import type { AuditEntry, AuditEvent, CoreEvent, Request, ResponseFor } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export const entry = (seq: number, event: AuditEvent, extra: Partial<AuditEntry> = {}): AuditEntry => ({
  seq,
  at_ms: Date.UTC(2026, 10, 24, 12, 0, seq),
  project: "p1",
  session: "s1",
  event,
  ...extra,
});

export const bash = (allowed: boolean): AuditEvent => ({
  kind: "tool_decided",
  tool: "Bash",
  input: { kind: "json", text: '{"command":"ls"}' },
  allowed,
  by: allowed ? { kind: "person" } : { kind: "policy", rule: "Bash(rm)" },
});

export class FakeAuditCore implements Transport {
  calls: Request[] = [];
  entries: AuditEntry[] = [];
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    if (request.method !== "audit.list") throw new Error(`unexpected ${request.method}`);
    const { project, after, limit } = request.params;
    const data = this.entries
      .filter((e) => e.project === project && e.seq > (after ?? 0))
      .slice(0, Math.min(limit ?? 500, 500));
    return { type: "audit.entries", data } as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  append(added: AuditEntry): void {
    this.entries.push(added);
    for (const listener of this.listeners) listener({ type: "audit.appended", data: added });
  }
}
