// What the open project's agents may reach and which tokens the netproxy adds
// for them, kept current from `netproxy.changed` and `secrets.changed`. Token
// values pass through here on their way to the core and are never kept.

import type {
  AllowedHost,
  CoreEvent,
  ProjectAllowlist,
  ProjectId,
  SecretInfo,
  SecretStoreStatus,
} from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class ProjectAccess {
  project = $state<ProjectId | null>(null);
  allowlist = $state<ProjectAllowlist | null>(null);
  secrets = $state<SecretInfo[]>([]);
  store = $state<SecretStoreStatus | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  private latest = 0;
  private readonly off: () => void;

  constructor(private readonly transport: Transport) {
    this.off = transport.onEvent((event) => this.apply(event));
  }

  dispose(): void {
    this.off();
  }

  /** Shows `project`'s access; null clears it. */
  async show(project: ProjectId | null): Promise<void> {
    if (project !== this.project) {
      this.allowlist = null;
      this.secrets = [];
      this.store = null;
    }
    this.project = project;
    await this.reload();
  }

  async reload(): Promise<void> {
    const project = this.project;
    const mine = ++this.latest;
    if (!project) {
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      const [allowlist, secrets] = await Promise.all([
        this.transport.call({ method: "netproxy.get_allowlist", params: { project } }),
        this.transport.call({ method: "secrets.list", params: { project } }),
      ]);
      if (mine !== this.latest) return;
      this.allowlist = allowlist.data;
      this.secrets = secrets.data.secrets;
      this.store = secrets.data.store;
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = message(err);
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  /** Replaces the project's own hosts; the answer is the list the core kept. */
  async setHosts(hosts: AllowedHost[]): Promise<void> {
    const project = this.project;
    if (!project) return;
    const kept = await this.transport.call({
      method: "netproxy.set_allowlist",
      params: { project, allowlist: { hosts } },
    });
    if (project === this.project) this.allowlist = kept.data;
  }

  /** Stores a token. `value` goes to the core and nowhere else. */
  async setSecret(secret: SecretInfo, value: string): Promise<void> {
    const project = this.project;
    if (!project) return;
    await this.transport.call({ method: "secrets.set", params: { project, secret, value } });
    await this.reload();
  }

  async removeSecret(name: string): Promise<void> {
    const project = this.project;
    if (!project) return;
    await this.transport.call({ method: "secrets.remove", params: { project, name } });
    await this.reload();
  }

  private apply(event: CoreEvent): void {
    if (event.type !== "netproxy.changed" && event.type !== "secrets.changed") return;
    if (event.data.project === this.project) void this.reload();
  }
}

export function message(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object" && "message" in err) return String((err as { message: unknown }).message);
  return String(err);
}
