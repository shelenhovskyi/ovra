// The open project's policy, kept current from `policy.changed`. Only the
// person changes it, through `save`; the core checks every rule.

import type { CoreEvent, Policy, PolicyRule, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class ProjectPolicy {
  project = $state<ProjectId | null>(null);
  /** The rules as the core keeps them; null until read. */
  rules = $state<PolicyRule[] | null>(null);
  /** Quiet mode (ADR 0015): what the sandbox contains runs without asking. */
  quiet = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  private latest = 0;
  private readonly off: (() => void)[];

  constructor(private readonly transport: Transport) {
    let wasOpen = false;
    this.off = [
      transport.onEvent((event) => this.apply(event)),
      transport.onStatus?.((status) => {
        if (status !== "open") return;
        // The policy may have changed while disconnected.
        if (wasOpen) void this.reload();
        wasOpen = true;
      }) ?? (() => {}),
    ];
  }

  dispose(): void {
    for (const off of this.off) off();
  }

  /** Shows `project`'s policy; null clears it. */
  async show(project: ProjectId | null): Promise<void> {
    if (project !== this.project) {
      this.rules = null;
      this.quiet = false;
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
      const reply = await this.transport.call({ method: "policy.get", params: { project } });
      if (mine !== this.latest) return;
      this.keep(reply.data);
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = reason(err);
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  /**
   * Replaces the project's rules with `rules`, built from `expected`. Throws
   * the core's reason when it refuses them, and `ChangedElsewhere` after
   * reading the fresh list when the policy changed since `expected`.
   */
  async save(rules: PolicyRule[], expected: PolicyRule[]): Promise<void> {
    await this.replace(policy(rules, this.quiet), policy(expected, this.quiet));
  }

  /** Turns quiet mode on or off, keeping the rules; throws as `save`. */
  async setQuiet(on: boolean): Promise<void> {
    const rules = this.rules;
    if (!rules) throw new Error("The policy is not read yet.");
    await this.replace(policy(rules, on), policy(rules, this.quiet));
  }

  private async replace(next: Policy, expected: Policy): Promise<void> {
    const project = this.project;
    if (!project) throw new Error("No project is open.");
    // The reply is kept only if no read started after the request was sent:
    // that read may know of a later change.
    const mine = this.latest;
    const reply = await this.set(project, next, expected).catch(async (err: unknown) => {
      if (!(err instanceof CoreError && err.code === "conflict")) throw err;
      if (project === this.project) await this.reload();
      throw new ChangedElsewhere();
    });
    if (project !== this.project || mine !== this.latest) return;
    // A read that started earlier must not overwrite what was just kept.
    this.latest++;
    this.loading = false;
    this.keep(reply.data);
  }

  private keep(data: Policy): void {
    this.rules = data.rules;
    this.quiet = data.quiet === true;
    this.error = null;
  }

  private set(project: ProjectId, next: Policy, expected: Policy) {
    return this.transport.call({ method: "policy.set", params: { project, policy: next, expected } });
  }

  private apply(event: CoreEvent): void {
    if (event.type === "policy.changed" && event.data.project === this.project) void this.reload();
  }
}

/** A policy as the core keeps it: `quiet` only when on. */
function policy(rules: PolicyRule[], quiet: boolean): Policy {
  return quiet ? { rules, quiet: true } : { rules };
}

/** A change built from an older policy; the fresh one is read already. */
export class ChangedElsewhere extends Error {
  constructor() {
    super("The policy changed elsewhere in the meantime. The list now shows it; check it and try again.");
  }
}

/** The core's own words for a refusal, without the error code. */
export function reason(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}
