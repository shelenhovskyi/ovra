// Test-only fake of the core's `terminals.*`, like `EchoTerminals` in
// `crates/core/src/fakes.rs`: ids are `t1`, `t2`, ..., input is echoed back as
// output, every output is kept as scrollback, and closing emits
// `terminals.exited` with code 0.

import type { CoreEvent, ProjectId, Request, ResponseFor, TermSize } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

type FakeTerminal = { project: ProjectId; size: TermSize; seq: number; scrollback: string; exited: boolean };

export class FakeTerminals implements Transport {
  calls: Request[] = [];
  open = new Map<string, FakeTerminal>();
  /** When set, `terminals.open` fails with this error. */
  openError: CoreError | null = null;
  /** When set, `terminals.open` waits for this promise before answering. */
  openGate: Promise<void> | null = null;
  /** When set, `terminals.scrollback` waits for this promise before answering. */
  scrollbackGate: Promise<void> | null = null;
  /** The project's files, for `fs.list_files`. */
  files: string[] = [];
  /** While true, every call fails at once, as the transport's do while the socket is closed. */
  down = false;
  private opened = 0;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    if (this.down) throw new Error("connection to core closed");
    return (await this.answer(request)) as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  /** The shell writes `data`; with `missed`, the client never sees the event (a lagging client). */
  output(id: string, data: string, missed = false): void {
    const terminal = this.get(id);
    terminal.seq += 1;
    terminal.scrollback += data;
    if (!missed) this.emit({ type: "terminals.output", data: { id, data, seq: terminal.seq } });
  }

  /** The shell ends on its own; the terminal stays known until closed. */
  exit(id: string, code: number | null): void {
    this.get(id).exited = true;
    this.emit({ type: "terminals.exited", data: { id, code } });
  }

  methods(): string[] {
    return this.calls.map((c) => c.method);
  }

  private get(id: string): FakeTerminal {
    const terminal = this.open.get(id);
    if (!terminal) throw new CoreError({ code: "not_found", message: `terminal ${id}` });
    return terminal;
  }

  private running(id: string): FakeTerminal {
    const terminal = this.get(id);
    if (terminal.exited) throw new CoreError({ code: "not_found", message: `terminal ${id} has ended` });
    return terminal;
  }

  private async answer(request: Request): Promise<unknown> {
    switch (request.method) {
      case "terminals.open": {
        if (this.openGate) await this.openGate;
        if (this.openError) throw this.openError;
        const id = `t${++this.opened}`;
        const { project, size } = request.params;
        this.open.set(id, { project, size, seq: 0, scrollback: "", exited: false });
        return { type: "terminals.opened", data: { id } };
      }
      case "terminals.input":
        this.running(request.params.id);
        this.output(request.params.id, request.params.data);
        return { type: "ok" };
      case "terminals.resize":
        this.running(request.params.id).size = request.params.size;
        return { type: "ok" };
      case "terminals.scrollback": {
        if (this.scrollbackGate) await this.scrollbackGate;
        const { scrollback, seq } = this.get(request.params.id);
        return { type: "terminals.scrollback", data: { data: scrollback, seq } };
      }
      case "terminals.close": {
        const { exited } = this.get(request.params.id);
        this.open.delete(request.params.id);
        if (!exited) this.emit({ type: "terminals.exited", data: { id: request.params.id, code: 0 } });
        return { type: "ok" };
      }
      case "fs.list_files":
        return { type: "fs.files", data: { paths: this.files, total: this.files.length, version: 1 } };
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }
}
