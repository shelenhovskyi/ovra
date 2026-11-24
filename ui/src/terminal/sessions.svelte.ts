// Terminal tabs of every project and their link to the core's `terminals.*`.
// A tab exists before its shell does: the view measures itself first, then
// `start` opens the shell at that size. Output that arrives while no view is
// attached is kept and written when one attaches.

import type { CoreEvent, ProjectId, TerminalId, TermSize } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export type SessionState =
  | { kind: "starting" }
  | { kind: "running"; id: TerminalId }
  | { kind: "exited"; code: number | null }
  | { kind: "failed"; message: string };

/** Where a session writes output: the xterm instance of its view. */
type Sink = (data: string) => void;

export class TerminalSession {
  state = $state<SessionState>({ kind: "starting" });
  /** Typed before the shell started; sent once it runs. */
  typedAhead = "";
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
    for (const chunk of this.pending.splice(0)) sink(chunk);
    return () => {
      if (this.sink === sink) this.sink = null;
    };
  }

  write(data: string): void {
    if (this.sink) this.sink(data);
    else this.pending.push(data);
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

export class TerminalSessions {
  sessions = $state<TerminalSession[]>([]);
  /** Active tab key per project. */
  private activeKeys = $state<Record<ProjectId, number>>({});
  private nextKey = 1;
  /** Highest "Terminal N" used per project, so numbers are not reused while tabs exist. */
  private numbers: Record<ProjectId, number> = {};

  constructor(private readonly transport: Transport) {}

  /** Starts listening for terminal events; returns a function that stops. */
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
    const id = session.id;
    if (id !== null) this.transport.call({ method: "terminals.close", params: { id } }).catch(() => {});
  }

  private find(id: TerminalId): TerminalSession | undefined {
    return this.sessions.find((s) => s.id === id);
  }

  private onEvent(event: CoreEvent): void {
    if (event.type === "terminals.output") {
      this.find(event.data.id)?.write(event.data.data);
    } else if (event.type === "terminals.exited") {
      const session = this.find(event.data.id);
      if (!session) return;
      session.state = { kind: "exited", code: event.data.code };
      session.write(`\r\n${exitNotice(event.data.code)}\r\n`);
    }
  }
}
