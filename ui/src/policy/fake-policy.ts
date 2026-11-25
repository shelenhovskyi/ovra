// Test-only fake of the core's `policy.*`: keeps each project's rules, refuses
// what a test tells it to with `bad_request`, refuses a `policy.set` built
// from an older policy with `conflict`, can hold a reply back, and lets a
// test push `policy.changed` and connection changes.

import type { CoreEvent, PolicyRule, Request, ResponseFor } from "../lib/protocol";
import { type ConnectionStatus, CoreError, type Transport } from "../lib/transport";

export class FakePolicyCore implements Transport {
  calls: Request[] = [];
  rules = new Map<string, PolicyRule[]>();
  /** A `policy.set` with a rule whose tool is named here is refused with the message. */
  refuse = new Map<string, string>();
  /** Replies to calls of this method wait until `release` is called. */
  hold: Request["method"] | null = null;
  private held: (() => void)[] = [];
  private listeners = new Set<(event: CoreEvent) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    // The core answers from the state it had when the request arrived.
    const reply = this.answer(request);
    if (request.method === this.hold) await new Promise<void>((resolve) => this.held.push(resolve));
    return reply();
  }

  /** Lets the held replies through. */
  release(): void {
    for (const resolve of this.held.splice(0)) resolve();
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  setStatus(status: ConnectionStatus): void {
    for (const listener of this.statusListeners) listener(status);
  }

  private answer<R extends Request>(request: R): () => ResponseFor<R> {
    switch (request.method) {
      case "policy.get": {
        const rules = this.reply(this.rules.get(request.params.project) ?? []);
        return () => rules as ResponseFor<R>;
      }
      case "policy.set": {
        const { project, policy, expected } = request.params;
        for (const rule of policy.rules) {
          const message = this.refuse.get(rule.tool);
          if (message) return fail({ code: "bad_request", message });
        }
        const stored = this.rules.get(project) ?? [];
        if (expected && JSON.stringify(expected.rules) !== JSON.stringify(stored)) {
          return fail({ code: "conflict", message: "the policy changed since this change was built" });
        }
        this.rules.set(project, policy.rules);
        this.emit({ type: "policy.changed", data: { project } });
        return () => this.reply(policy.rules) as ResponseFor<R>;
      }
      default:
        return fail({ code: "internal", message: `unexpected ${request.method}` });
    }
  }

  private reply(rules: PolicyRule[]) {
    return { type: "policy.policy", data: { rules: [...rules] } };
  }
}

function fail(error: ConstructorParameters<typeof CoreError>[0]): () => never {
  return () => {
    throw new CoreError(error);
  };
}
