// The open project's policy, kept current from `policy.changed`. Only the
// person changes it, through `save`; the core checks every rule.

import type { CoreEvent, PolicyRule, ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class ProjectPolicy {
  project = $state<ProjectId | null>(null);
  /** The rules as the core keeps them; null until read. */
  rules = $state<PolicyRule[] | null>(null);
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
    if (project !== this.project) this.rules = null;
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
      this.rules = reply.data.rules;
      this.error = null;
    } catch (err) {
      if (mine !== this.latest) return;
      this.error = reason(err);
    } finally {
      if (mine === this.latest) this.loading = false;
    }
  }

  /** Replaces the project's rules; throws the core's reason when it refuses them. */
  async save(rules: PolicyRule[]): Promise<void> {
    const project = this.project;
    if (!project) throw new Error("No project is open.");
    const reply = await this.transport.call({ method: "policy.set", params: { project, policy: { rules } } });
    if (project !== this.project) return;
    // A read that started earlier must not overwrite what was just kept.
    this.latest++;
    this.loading = false;
    this.rules = reply.data.rules;
    this.error = null;
  }

  private apply(event: CoreEvent): void {
    if (event.type === "policy.changed" && event.data.project === this.project) void this.reload();
  }
}

/** The core's own words for a refusal, without the error code. */
export function reason(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}
