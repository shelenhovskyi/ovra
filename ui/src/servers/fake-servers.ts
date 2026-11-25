// Test-only fake of the core's `servers.*`: definitions and servers a test
// sets, starts that answer `starting`, and logs kept per server. Tests send
// status and output events themselves with `emit`.

import type {
  CoreEvent,
  LogLine,
  ProtocolError,
  Request,
  ResponseFor,
  ServerDef,
  DevServerInfo as ServerInfo,
  SessionInfo,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class FakeServersCore implements Transport {
  calls: Request[] = [];
  definitions: ServerDef[] = [{ name: "web", command: ["pnpm", "dev"], port: 5173 }];
  servers: ServerInfo[] = [];
  fileError: string | null = null;
  logs = new Map<string, LogLine[]>();
  sessions: SessionInfo[] = [];
  /** Project files by path, for `fs.read_file` and `fs.write_file`. */
  files = new Map<string, { text: string; mtime_ms: number }>();
  /** Errors to answer with, by method. */
  refuse = new Map<string, ProtocolError>();
  private nextId = 1;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    const refused = this.refuse.get(request.method);
    if (refused) throw new CoreError(refused);
    switch (request.method) {
      case "agents.list":
        return { type: "agents.list", data: this.sessions } as ResponseFor<R>;
      case "orchestration.list":
        return { type: "orchestration.list", data: [] } as unknown as ResponseFor<R>;
      case "servers.list": {
        const workDir = request.params.work_dir ?? null;
        const data = {
          definitions: this.definitions,
          servers: this.servers.filter((s) => s.work_dir === workDir),
          error: this.fileError,
        };
        return { type: "servers.list", data } as ResponseFor<R>;
      }
      case "servers.overview":
        return { type: "servers.overview", data: { servers: this.servers } } as ResponseFor<R>;
      case "servers.start": {
        const { project, name, work_dir } = request.params;
        const def = this.definitions.find((d) => d.name === name);
        if (!def) throw new CoreError({ code: "not_found", message: `no server ${name}` });
        const info: ServerInfo = {
          id: `srv${this.nextId++}`,
          project,
          name,
          work_dir: work_dir ?? null,
          status: { state: "starting" },
          generation: 1,
          port: def.port,
          started_by: { kind: "person" },
        };
        this.servers.push(info);
        return { type: "servers.server", data: info } as ResponseFor<R>;
      }
      case "fs.read_file": {
        const { path } = request.params;
        const file = this.files.get(path);
        if (!file) throw new CoreError({ code: "not_found", message: `${path}: No such file or directory` });
        const data = { path, size: file.text.length, mtime_ms: file.mtime_ms, body: { kind: "text", text: file.text } };
        return { type: "fs.file", data } as ResponseFor<R>;
      }
      case "fs.write_file": {
        const { path, text, expected_mtime_ms } = request.params;
        const known = this.files.get(path);
        // Like the core: a time given must be the file's; none writes whatever is there.
        if (expected_mtime_ms !== null && known?.mtime_ms !== expected_mtime_ms)
          throw new CoreError({ code: "conflict", message: `${path} changed on disk` });
        const mtime_ms = (known?.mtime_ms ?? 0) + 1;
        this.files.set(path, { text, mtime_ms });
        // The core reads the servers file again on the next list.
        if (path === ".ovra/servers.json") this.definitions = JSON.parse(text).servers;
        return { type: "fs.written", data: { mtime_ms } } as ResponseFor<R>;
      }
      case "servers.stop":
        return { type: "ok" } as ResponseFor<R>;
      case "servers.restart": {
        const known = this.servers.find((s) => s.id === request.params.id);
        if (!known) throw new CoreError({ code: "not_found", message: "no such server" });
        Object.assign(known, { status: { state: "starting" }, generation: known.generation + 1 });
        return { type: "servers.server", data: { ...known } } as ResponseFor<R>;
      }
      case "servers.logs": {
        const { id, since, limit } = request.params;
        const all = (this.logs.get(id) ?? []).filter((l) => l.n > (since ?? 0));
        const lines = all.slice(0, limit ?? all.length);
        const next = lines.at(-1)?.n ?? since ?? 0;
        return { type: "servers.logs", data: { lines, next } } as ResponseFor<R>;
      }
      default:
        throw new Error(`unexpected ${request.method}`);
    }
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }
}
