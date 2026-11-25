// Test-only fake of the core's `netproxy.*` and `secrets.*`: keeps one
// project's hosts and secrets, checks nothing but what a test asks it to.

import type { AllowedHost, CoreEvent, Request, ResponseFor, SecretInfo, SecretStoreStatus } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class FakeAccessCore implements Transport {
  calls: Request[] = [];
  base: AllowedHost[] = [{ host: "api.anthropic.com" }];
  hosts: AllowedHost[] = [];
  broad = new Set(["gist.github.com"]);
  secrets: SecretInfo[] = [];
  /** Values the core received, so a test can check where they went. */
  values = new Map<string, string>();
  store: SecretStoreStatus = { state: "available", name: "Secret Service" };
  refuse: string | null = null;
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    if (this.refuse) throw new Error(this.refuse);
    switch (request.method) {
      case "netproxy.get_allowlist":
        return this.allowlist() as ResponseFor<R>;
      case "netproxy.set_allowlist":
        this.hosts = request.params.allowlist.hosts;
        return this.allowlist() as ResponseFor<R>;
      case "secrets.list":
        return { type: "secrets.list", data: { store: this.store, secrets: [...this.secrets] } } as ResponseFor<R>;
      case "secrets.set": {
        const { secret, value } = request.params;
        this.secrets = [...this.secrets.filter((s) => s.name !== secret.name), secret];
        this.values.set(secret.name, value);
        return { type: "ok" } as ResponseFor<R>;
      }
      case "secrets.remove":
        this.secrets = this.secrets.filter((s) => s.name !== request.params.name);
        return { type: "ok" } as ResponseFor<R>;
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

  private allowlist() {
    return {
      type: "netproxy.allowlist",
      data: {
        base: { hosts: this.base },
        project: { hosts: this.hosts },
        broad: this.hosts.map((h) => h.host).filter((h) => this.broad.has(h)),
      },
    };
  }
}
