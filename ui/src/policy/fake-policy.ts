// Test-only fake of the core's `policy.*`: keeps each project's rules, refuses
// what a test tells it to with `bad_request`, and lets a test push
// `policy.changed`.

import type { CoreEvent, PolicyRule, Request, ResponseFor } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class FakePolicyCore implements Transport {
  calls: Request[] = [];
  rules = new Map<string, PolicyRule[]>();
  /** A `policy.set` with a rule whose tool is named here is refused with the message. */
  refuse = new Map<string, string>();
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    switch (request.method) {
      case "policy.get":
        return this.reply(this.rules.get(request.params.project) ?? []) as ResponseFor<R>;
      case "policy.set": {
        const { project, policy } = request.params;
        for (const rule of policy.rules) {
          const message = this.refuse.get(rule.tool);
          if (message) throw new CoreError({ code: "bad_request", message });
        }
        this.rules.set(project, policy.rules);
        this.emit({ type: "policy.changed", data: { project } });
        return this.reply(policy.rules) as ResponseFor<R>;
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

  private reply(rules: PolicyRule[]) {
    return { type: "policy.policy", data: { rules: [...rules] } };
  }
}
