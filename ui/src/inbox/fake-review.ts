// Test-only fake of the core's review requests: records calls, answers them
// like the core (task 3.B) and lets a test push events.

import type { CoreEvent, FileChange, Request, ResponseFor, ReviewItem } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export const change = (path: string, extra: Partial<FileChange> = {}): FileChange => ({
  path,
  kind: "modified",
  from: null,
  by: "agent",
  additions: 1,
  deletions: 1,
  blocked: false,
  ...extra,
});

export const review = (id: string, extra: Partial<ReviewItem> = {}): ReviewItem => ({
  id,
  project: "p1",
  session: "s1",
  mode: "direct",
  state: "open",
  summary: null,
  files: [change("README.md")],
  can_undo: false,
  not_copied: [],
  can_undo_take: false,
  ...extra,
});

const patchFor = (path: string): string => `--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-teh readme\n+the readme\n`;

export class FakeReviewCore implements Transport {
  calls: Request[] = [];
  items: ReviewItem[] = [];
  /** What approve and reject report as left for the person. */
  conflicts: string[] = [];
  /** What reject reports as restored and undo as put back. */
  restored: string[] = [];
  /** Paths whose diff the core no longer keeps (a decided item past the limits): `not_found` with this reason. */
  diffGone = new Map<string, string>();
  /** Makes the next call of this method fail with this error. */
  failNext = new Map<Request["method"], CoreError>();
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    const failure = this.failNext.get(request.method);
    if (failure) {
      this.failNext.delete(request.method);
      throw failure;
    }
    return this.answer(request) as ResponseFor<R>;
  }

  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  sent<M extends Request["method"]>(method: M): Extract<Request, { method: M }>[] {
    return this.calls.filter((c): c is Extract<Request, { method: M }> => c.method === method);
  }

  private find(id: string): ReviewItem {
    const item = this.items.find((i) => i.id === id);
    if (!item) throw new CoreError({ code: "not_found", message: `no review ${id}` });
    return item;
  }

  private answer(request: Request): unknown {
    switch (request.method) {
      case "review.list":
        // Copies, as over the wire: later changes here must not reach the UI's items.
        return {
          type: "review.list",
          data: structuredClone(this.items.filter((i) => i.project === request.params.project)),
        };
      case "review.diff":
        this.find(request.params.id);
        {
          const gone = this.diffGone.get(request.params.path);
          if (gone) throw new CoreError({ code: "not_found", message: gone });
        }
        return { type: "review.diff", data: { path: request.params.path, patch: patchFor(request.params.path) } };
      case "review.approve":
      case "review.reject":
      case "review.undo_reject": {
        const item = this.find(request.params.id);
        const state = {
          "review.approve": "approved",
          "review.reject": "rejected",
          "review.undo_reject": "open",
        } as const;
        item.state = state[request.method];
        // Like the core: a worker's approve that left files is partial.
        if (request.method === "review.approve" && item.mode === "isolated" && this.conflicts.length > 0) {
          item.state = "partly_approved";
          item.not_copied = [...this.conflicts];
        }
        item.can_undo = request.method === "review.reject";
        const restored = request.method === "review.approve" ? [] : [...this.restored];
        return { type: "review.done", data: { item: structuredClone(item), conflicts: [...this.conflicts], restored } };
      }
      case "review.take": {
        const item = this.find(request.params.id);
        const taken = item.not_copied.filter((p) => !this.conflicts.includes(p));
        item.not_copied = item.not_copied.filter((p) => !taken.includes(p));
        if (item.not_copied.length === 0) item.state = "approved";
        item.can_undo_take = taken.length > 0;
        return {
          type: "review.done",
          data: { item: structuredClone(item), conflicts: [...this.conflicts], restored: taken },
        };
      }
      case "review.undo_take": {
        const item = this.find(request.params.id);
        item.not_copied = [...item.not_copied, ...this.restored].sort();
        item.state = "partly_approved";
        item.can_undo_take = false;
        return {
          type: "review.done",
          data: { item: structuredClone(item), conflicts: [], restored: [...this.restored] },
        };
      }
      case "review.resolve": {
        const item = this.find(request.params.id);
        item.state = "approved";
        return { type: "review.done", data: { item: structuredClone(item), conflicts: [], restored: [] } };
      }
      case "review.comment":
        this.find(request.params.id);
        return { type: "ok" };
      case "agents.list":
        return { type: "agents.list", data: [] };
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }
}
