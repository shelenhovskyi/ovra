// The open project's servers for one work dir (the main checkout or a
// worker's worktree): definitions from `.ovra/servers.json` and the servers
// that ran, kept current from `servers.status`.

import type {
  CoreEvent,
  ProjectId,
  DevServerId as ServerId,
  DevServerInfo as ServerInfo,
  ServerList,
  ServerStatus,
  SessionId,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class ProjectServers {
  project = $state<ProjectId | null>(null);
  workDir = $state<SessionId | null>(null);
  list = $state<ServerList | null>(null);
  /** Why servers cannot run here at all (no Linux sandbox). */
  unavailable = $state<string | null>(null);
  /** The last call that failed. */
  error = $state<string | null>(null);
  private latest = 0;
  private readonly off: () => void;

  constructor(private readonly transport: Transport) {
    this.off = transport.onEvent((event) => this.apply(event));
  }

  dispose(): void {
    this.off();
  }

  async show(project: ProjectId | null, workDir: SessionId | null): Promise<void> {
    if (project !== this.project || workDir !== this.workDir) this.list = null;
    this.project = project;
    this.workDir = workDir;
    await this.reload();
  }

  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) return;
    try {
      const { data } = await this.transport.call({
        method: "servers.list",
        params: { project, work_dir: this.workDir },
      });
      if (mine !== this.latest) return;
      this.list = data;
      this.unavailable = null;
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      if (err instanceof CoreError && err.code === "unavailable") this.unavailable = err.error.message;
      else this.error = message(err);
    }
  }

  /** Starts definition `name`; answers the new server, or null when the call failed. */
  async start(name: string): Promise<ServerInfo | null> {
    const project = this.project;
    if (!project) return null;
    return this.act(() =>
      this.transport.call({ method: "servers.start", params: { project, name, work_dir: this.workDir } }),
    );
  }

  async stop(id: ServerId): Promise<ServerInfo | null> {
    return this.act(async () => {
      await this.transport.call({ method: "servers.stop", params: { id } });
      return null;
    });
  }

  async restart(id: ServerId): Promise<ServerInfo | null> {
    return this.act(() => this.transport.call({ method: "servers.restart", params: { id } }));
  }

  /** The newest server started from definition `name`, if any. */
  latestOf(name: string): ServerInfo | undefined {
    return this.list?.servers.filter((s) => s.name === name).at(-1);
  }

  private async act(call: () => Promise<{ data: ServerInfo } | null>): Promise<ServerInfo | null> {
    try {
      const reply = await call();
      if (reply) this.upsert(reply.data);
      this.error = null;
      return reply?.data ?? null;
    } catch (err) {
      if (err instanceof CoreError && err.code === "unavailable") this.unavailable = err.error.message;
      else this.error = message(err);
      return null;
    }
  }

  private upsert(info: ServerInfo): void {
    if (!this.list || info.project !== this.project) return;
    const others = this.list.servers.filter((s) => s.id !== info.id);
    this.list = { ...this.list, servers: [...others, info] };
  }

  private apply(event: CoreEvent): void {
    if (event.type === "info.resync") {
      void this.reload();
      return;
    }
    if (event.type !== "servers.status" || event.data.project !== this.project || !this.list) return;
    const { id, status, generation } = event.data;
    const known = this.list.servers.find((s) => s.id === id);
    if (known) this.upsert({ ...known, status, generation });
    // A server someone else started (an agent, another window).
    else if (status.state === "starting") void this.reload();
  }
}

/** `ready`, `exited (code 1)`, `failed: port 5173 is taken` */
export function statusText(status: ServerStatus): string {
  switch (status.state) {
    case "exited":
      return status.code === null ? "exited (signal)" : `exited (code ${status.code})`;
    case "failed":
      return `failed: ${status.reason}`;
    default:
      return status.state;
  }
}

export function running(status: ServerStatus): boolean {
  return status.state === "starting" || status.state === "ready" || status.state === "stopping";
}

export function message(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  if (err instanceof Error) return err.message;
  return String(err);
}
