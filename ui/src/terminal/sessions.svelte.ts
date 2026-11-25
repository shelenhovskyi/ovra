// Terminal tabs of every project and their link to the core's `terminals.*`.
// A tab exists before its shell does: the view measures itself first, then
// `start` opens the shell at that size. Output that arrives while no view is
// attached is kept and written when one attaches.
//
// Output events carry a per-terminal `seq`. When events were missed (an
// `info.resync`, a reconnect, or a gap in `seq`) the tab is redrawn from the
// core's scrollback, and events already in it are dropped.

import type { CoreEvent, ProjectId, TerminalId, TermSize } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export type SessionState =
  | { kind: "starting" }
  | { kind: "running"; id: TerminalId }
  | { kind: "exited"; code: number | null }
  /** The core no longer knows the terminal, e.g. after it restarted. */
  | { kind: "lost" }
  | { kind: "failed"; message: string };

/** Where a session writes output: the xterm instance of its view. */
type Sink = { write(data: string): void; reset(): void };

type Output = { data: string; seq: number };

export class TerminalSession {
  state = $state<SessionState>({ kind: "starting" });
  /** Typed before the shell started; sent once it runs. */
  typedAhead = "";
  /** The core's id, kept after the shell ends so the tab can still be redrawn and closed. */
  coreId: TerminalId | null = null;
  /** Last output event written. */
  seq = 0;
  /** Events that arrived while a redraw was loading. */
  held: Output[] | null = null;
  private sink: Sink | null = null;
  private pending: string[] = [];

  constructor(
    /** Local key, unique in this window; stable from creation, unlike the core's id. */
    readonly key: number,
    readonly project: ProjectId,
    readonly title: string,
  ) {}

  get id(): TerminalId | null {
    return this.state.kind === "running" ? this.state.id : null;
  }

  /** Attaches the view's writer and flushes held output; returns a detach function. */
  attach(sink: Sink): () => void {
    this.sink = sink;
    for (const chunk of this.pending.splice(0)) sink.write(chunk);
    return () => {
      if (this.sink === sink) this.sink = null;
    };
  }

  write(data: string): void {
    if (this.sink) this.sink.write(data);
    else this.pending.push(data);
  }

  /** Clears the screen and scrollback, before a redraw. */
  reset(): void {
    this.pending = [];
    this.sink?.reset();
  }
}

/** The message of a failed call, without the error code prefix. */
function failure(error: unknown): string {
  if (error instanceof CoreError) return error.error.message;
  return error instanceof Error ? error.message : String(error);
}

export function exitNotice(code: number | null): string {
  return code === null ? "Process was killed." : `Process exited with code ${code}.`;
}

export const LOST_NOTICE = "This terminal is gone: the core no longer knows it, probably after a restart.";

export class TerminalSessions {
  sessions = $state<TerminalSession[]>([]);
  /** Active tab key per project. */
  private activeKeys = $state<Record<ProjectId, number>>({});
  private nextKey = 1;
  /** Highest "Terminal N" used per project, so numbers are not reused while tabs exist. */
  private numbers: Record<ProjectId, number> = {};

  constructor(private readonly transport: Transport) {}

  /** Starts listening for terminal and resync events; returns a function that stops. */
  listen(): () => void {
    return this.transport.onEvent((event) => this.onEvent(event));
  }

  of(project: ProjectId): TerminalSession[] {
    return this.sessions.filter((s) => s.project === project);
  }

  active(project: ProjectId): TerminalSession | undefined {
    const key = this.activeKeys[project];
    return this.sessions.find((s) => s.key === key && s.project === project);
  }

  activate(session: TerminalSession): void {
    this.activeKeys[session.project] = session.key;
  }

  /** Adds a tab in `project` and makes it active; its view calls `start`. */
  add(project: ProjectId): TerminalSession {
    const count = this.of(project).length === 0 ? 1 : (this.numbers[project] ?? 0) + 1;
    this.numbers[project] = count;
    const session = new TerminalSession(this.nextKey++, project, `Terminal ${count}`);
    this.sessions.push(session);
    this.activate(session);
    return session;
  }

  /** Opens the shell for a tab at the size its view measured. */
  async start(session: TerminalSession, size: TermSize): Promise<void> {
    if (session.state.kind !== "starting") return;
    try {
      const { data } = await this.transport.call({
        method: "terminals.open",
        params: { project: session.project, size },
      });
      if (!this.sessions.includes(session)) {
        // Closed while starting: end the shell nobody will see.
        await this.transport.call({ method: "terminals.close", params: { id: data.id } }).catch(() => {});
        return;
      }
      session.coreId = data.id;
      session.state = { kind: "running", id: data.id };
      const typed = session.typedAhead;
      session.typedAhead = "";
      if (typed) this.input(session, typed);
    } catch (error) {
      session.state = { kind: "failed", message: failure(error) };
    }
  }

  input(session: TerminalSession, data: string): void {
    if (session.state.kind === "starting") session.typedAhead += data;
    const id = session.id;
    if (id === null) return;
    this.transport.call({ method: "terminals.input", params: { id, data } }).catch((error: unknown) => {
      session.write(`\r\n${failure(error)}\r\n`);
    });
  }

  resize(session: TerminalSession, size: TermSize): void {
    const id = session.id;
    if (id === null) return;
    this.transport.call({ method: "terminals.resize", params: { id, size } }).catch(() => {});
  }

  /** Removes the tab, ending its shell if it still runs; activates a neighbour. */
  close(session: TerminalSession): void {
    const siblings = this.of(session.project);
    const index = siblings.indexOf(session);
    if (index < 0) return;
    this.sessions = this.sessions.filter((s) => s !== session);
    if (this.activeKeys[session.project] === session.key) {
      const next = siblings[index + 1] ?? siblings[index - 1];
      if (next && next !== session) this.activate(next);
      else delete this.activeKeys[session.project];
    }
    this.forget(session);
  }

  /** Replaces a tab with a new shell under the same title, in the same place, and activates it. */
  restart(session: TerminalSession): TerminalSession | undefined {
    const index = this.sessions.indexOf(session);
    if (index < 0) return undefined;
    const fresh = new TerminalSession(this.nextKey++, session.project, session.title);
    this.sessions[index] = fresh;
    this.activate(fresh);
    this.forget(session);
    return fresh;
  }

  /** Closes a removed tab's shell, also an ended one, so the core can forget its scrollback. */
  private forget(session: TerminalSession): void {
    const id = session.coreId;
    if (id !== null && session.state.kind !== "lost") {
      this.transport.call({ method: "terminals.close", params: { id } }).catch(() => {});
    }
  }

  /** Redraws a tab from the core's scrollback; output arriving meanwhile is held and applied after. */
  async redraw(session: TerminalSession): Promise<void> {
    const id = session.coreId;
    if (id === null || session.held || session.state.kind === "lost") return;
    session.held = [];
    try {
      const { data } = await this.transport.call({ method: "terminals.scrollback", params: { id } });
      session.reset();
      session.write(data.data);
      session.seq = data.seq;
      if (session.state.kind === "exited") session.write(`\r\n${exitNotice(session.state.code)}\r\n`);
    } catch (error) {
      if (error instanceof CoreError && error.code === "not_found") {
        session.state = { kind: "lost" };
        session.write(`\r\n${LOST_NOTICE}\r\n`);
      }
      // Otherwise (connection closed) the next resync tries again.
    }
    const held = session.held;
    session.held = null;
    if (session.state.kind === "lost") return;
    for (const output of held) this.output(session, output);
  }

  private find(id: TerminalId): TerminalSession | undefined {
    return this.sessions.find((s) => s.coreId === id);
  }

  private output(session: TerminalSession, output: Output): void {
    if (session.held) {
      session.held.push(output);
      return;
    }
    if (output.seq <= session.seq) return; // already in a redraw
    if (output.seq > session.seq + 1) {
      // Missed events: the redraw brings them and this one.
      void this.redraw(session);
      return;
    }
    session.seq = output.seq;
    session.write(output.data);
  }

  private onEvent(event: CoreEvent): void {
    if (event.type === "terminals.output") {
      const session = this.find(event.data.id);
      if (session) this.output(session, event.data);
    } else if (event.type === "terminals.exited") {
      const session = this.find(event.data.id);
      if (!session || session.state.kind === "lost") return;
      session.state = { kind: "exited", code: event.data.code };
      // During a redraw the notice is written after the scrollback.
      if (!session.held) session.write(`\r\n${exitNotice(event.data.code)}\r\n`);
    } else if (event.type === "info.resync") {
      for (const session of this.sessions) void this.redraw(session);
    }
  }
}
