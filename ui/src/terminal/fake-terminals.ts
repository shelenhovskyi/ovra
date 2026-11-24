// Test-only fake of the core's `terminals.*`, like `EchoTerminals` in
// `crates/core/src/fakes.rs`: ids are `t1`, `t2`, ..., input is echoed back as
// output and closing emits `terminals.exited` with code 0.

import type { CoreEvent, ProjectId, Request, ResponseFor, TermSize } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class FakeTerminals implements Transport {
  calls: Request[] = [];
  open = new Map<string, { project: ProjectId; size: TermSize }>();
  /** When set, `terminals.open` fails with this error. */
  openError: CoreError | null = null;
  /** When set, `terminals.open` waits for this promise before answering. */
  openGate: Promise<void> | null = null;
  private opened = 0;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    return (await this.answer(request)) as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  methods(): string[] {
    return this.calls.map((c) => c.method);
  }

  private known(id: string): void {
    if (!this.open.has(id)) throw new CoreError({ code: "not_found", message: `terminal ${id}` });
  }

  private async answer(request: Request): Promise<unknown> {
    switch (request.method) {
      case "terminals.open": {
        if (this.openGate) await this.openGate;
        if (this.openError) throw this.openError;
        const id = `t${++this.opened}`;
        this.open.set(id, { project: request.params.project, size: request.params.size });
        return { type: "terminals.opened", data: { id } };
      }
      case "terminals.input":
        this.known(request.params.id);
        this.emit({ type: "terminals.output", data: { id: request.params.id, data: request.params.data } });
        return { type: "ok" };
      case "terminals.resize": {
        this.known(request.params.id);
        const terminal = this.open.get(request.params.id);
        if (terminal) terminal.size = request.params.size;
        return { type: "ok" };
      }
      case "terminals.close":
        this.known(request.params.id);
        this.open.delete(request.params.id);
        this.emit({ type: "terminals.exited", data: { id: request.params.id, code: 0 } });
        return { type: "ok" };
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }
}
