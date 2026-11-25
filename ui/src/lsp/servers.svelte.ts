// The open project's language servers (ADR 0007, docs/specs/lsp.md): loaded
// with `lsp.status`, kept current from `lsp.changed`. One instance per
// transport (`languageServers(transport)`), so the editor's client can report
// which languages it needed a server for.

import { SvelteSet } from "svelte/reactivity";
import type { CoreEvent, KnownServer, LspStatus, ProjectId, ServerId, ServerInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class LanguageServerStatus {
  project = $state<ProjectId | null>(null);
  /** The last `lsp.status` answer, null until one arrives. */
  status = $state<Omit<LspStatus, "servers"> | null>(null);
  /** The project's servers that have not stopped, failed ones with their reason. */
  servers = $state<ServerInfo[]>([]);
  error = $state<string | null>(null);
  /** Languages the editor needed a server for. Servers are found on `PATH`, so this holds for every project. */
  readonly needed = new SvelteSet<string>();
  private latest = 0;

  /** Known servers that are not installed. */
  readonly missing = $derived<KnownServer[]>(this.status?.known.filter((k) => k.path === null) ?? []);
  /** Missing servers for languages the editor needed. */
  readonly hints = $derived(this.missing.filter((k) => this.needed.has(k.language)));

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
  }

  /** Shows `project`'s servers; null clears them. */
  async show(project: ProjectId | null): Promise<void> {
    if (project !== this.project) {
      this.status = null;
      this.servers = [];
      this.error = null;
    }
    this.project = project;
    await this.reload();
  }

  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) return;
    try {
      const { data } = await this.transport.call({ method: "lsp.status", params: { project } });
      if (mine !== this.latest) return;
      const { servers, ...rest } = data;
      this.status = rest;
      this.servers = servers.filter((s) => s.state !== "stopped");
      this.error = null;
    } catch (err) {
      if (mine === this.latest) this.error = message(err);
    }
  }

  /** Called by the editor when it wanted a server for `language`: a missing one gets a hint. */
  needServer(language: string): void {
    this.needed.add(language);
  }

  async stop(server: ServerId): Promise<void> {
    await this.transport.call({ method: "lsp.stop", params: { server } });
  }

  private apply(event: CoreEvent): void {
    if (event.type === "info.resync") {
      void this.reload();
      return;
    }
    if (event.type !== "lsp.changed") return;
    const server = event.data.server;
    if (server.project !== this.project) return;
    // A new start for a language replaces its failed server.
    const others = this.servers.filter(
      (s) =>
        s.id !== server.id && !(server.state === "starting" && s.language === server.language && s.state === "failed"),
    );
    if (server.state === "stopped") {
      this.servers = others;
      return;
    }
    // A known server keeps its place in the list.
    this.servers = this.servers.some((s) => s.id === server.id)
      ? this.servers.flatMap((s) => (s.id === server.id ? [server] : others.includes(s) ? [s] : []))
      : [...others, server];
  }
}

function message(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) return String((err as { message: unknown }).message);
  return String(err);
}

const instances = new WeakMap<Transport, LanguageServerStatus>();

export function languageServers(transport: Transport): LanguageServerStatus {
  let servers = instances.get(transport);
  if (!servers) {
    servers = new LanguageServerStatus(transport);
    instances.set(transport, servers);
  }
  return servers;
}
