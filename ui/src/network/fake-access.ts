// Test-only fake of the core's `netproxy.*`, `secrets.*` and `policy.*`: keeps
// one project's hosts, secrets and rules, checks nothing but what a test asks it to.

import type {
  AllowedHost,
  CoreEvent,
  PolicyRule,
  Request,
  ResponseFor,
  SecretInfo,
  SecretStoreStatus,
  SessionInfo,
} from "../lib/protocol";
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
  sessions: SessionInfo[] = [];
  rules: PolicyRule[] = [];
  /** Errors to answer with, by method. */
  refuse = new Map<string, string>();
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    const refused = this.refuse.get(request.method);
    if (refused) throw new Error(refused);
    switch (request.method) {
      case "agents.list":
        return { type: "agents.list", data: this.sessions } as ResponseFor<R>;
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
      case "policy.get":
        return { type: "policy.policy", data: { rules: [...this.rules] } } as ResponseFor<R>;
      case "policy.set":
        this.rules = request.params.policy.rules;
        return { type: "policy.policy", data: { rules: [...this.rules] } } as ResponseFor<R>;
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
