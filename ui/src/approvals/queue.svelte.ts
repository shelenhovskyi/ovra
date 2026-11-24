// Permission requests waiting for a person. One queue per transport
// (`approvalQueue(transport)`); the core is the source of truth and nothing
// here ever answers on its own.

import { SvelteMap, SvelteSet } from "svelte/reactivity";
import type { CoreEvent, Decision, PermissionId, PermissionRequest } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/**
 * A request cannot be answered until this long after it appeared and after the
 * previous answer, so a key press or click meant for something else (or a
 * double press) never approves it.
 */
export const ARM_MS = 500;

export class ApprovalQueue {
  /** Oldest first. */
  pending = $state<PermissionRequest[]>([]);
  /** When each pending request arrived, `Date.now()`. */
  readonly arrived = new SvelteMap<PermissionId, number>();
  /** Requests with an answer on the way; they cannot be answered again. */
  readonly answering = new SvelteSet<PermissionId>();
  error = $state<string | null>(null);

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
    if (transport.onStatus) {
      transport.onStatus((status) => {
        if (status === "open") void this.load();
      });
    } else {
      void this.load();
    }
  }

  async load(): Promise<void> {
    try {
      const { data } = await this.transport.call({ method: "permissions.pending" });
      const now = Date.now();
      for (const request of data) if (!this.arrived.has(request.id)) this.arrived.set(request.id, now);
      this.pending = data;
      this.error = null;
    } catch (err) {
      this.error = `Could not load approvals: ${message(err)}`;
    }
  }

  /** Answers one request; `false` if it was not sent (already answering, or failed). */
  async answer(id: PermissionId, decision: Decision, note?: string): Promise<boolean> {
    if (this.answering.has(id) || !this.pending.some((r) => r.id === id)) return false;
    this.answering.add(id);
    try {
      const params = note ? { id, decision, message: note } : { id, decision };
      await this.transport.call({ method: "permissions.answer", params });
      this.remove(id);
      this.error = null;
      return true;
    } catch (err) {
      // Answered elsewhere or its session ended: it is gone either way.
      if (err instanceof CoreError && err.code === "not_found") this.remove(id);
      else this.error = `Could not send the answer: ${message(err)}`;
      return false;
    } finally {
      this.answering.delete(id);
    }
  }

  private apply(event: CoreEvent): void {
    if (event.type === "permissions.requested") {
      if (this.pending.some((r) => r.id === event.data.id)) return;
      this.arrived.set(event.data.id, Date.now());
      this.pending = [...this.pending, event.data];
    } else if (event.type === "permissions.resolved") {
      this.remove(event.data.id);
    }
  }

  private remove(id: PermissionId): void {
    this.pending = this.pending.filter((r) => r.id !== id);
    this.arrived.delete(id);
  }
}

function message(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

const instances = new WeakMap<Transport, ApprovalQueue>();

export function approvalQueue(transport: Transport): ApprovalQueue {
  let queue = instances.get(transport);
  if (!queue) {
    queue = new ApprovalQueue(transport);
    instances.set(transport, queue);
  }
  return queue;
}
