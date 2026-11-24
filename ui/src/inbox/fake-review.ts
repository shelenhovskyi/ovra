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
  ...extra,
});

const patchFor = (path: string): string => `--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-teh readme\n+the readme\n`;

export class FakeReviewCore implements Transport {
  calls: Request[] = [];
  items: ReviewItem[] = [];
  /** What approve and reject report as left for the person. */
  conflicts: string[] = [];
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
        return { type: "review.list", data: this.items.filter((i) => i.project === request.params.project) };
      case "review.diff":
        this.find(request.params.id);
        return { type: "review.diff", data: { path: request.params.path, patch: patchFor(request.params.path) } };
      case "review.approve":
      case "review.reject": {
        const item = this.find(request.params.id);
        item.state = request.method === "review.approve" ? "approved" : "rejected";
        return { type: "review.done", data: { item: { ...item }, conflicts: [...this.conflicts] } };
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
