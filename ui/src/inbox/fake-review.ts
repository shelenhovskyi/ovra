// Test-only fake of the core's review requests: records calls, answers them
// like the core (task 3.B) and lets a test push events.

import type {
  AgentMessage,
  CoreEvent,
  FileChange,
  GhStatus,
  Project,
  PrStatus,
  Request,
  ResponseFor,
  ReviewItem,
  ReviewWarning,
  SessionInfo,
  Task,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export const change = (path: string, extra: Partial<FileChange> = {}): FileChange => ({
  path,
  kind: "modified",
  from: null,
  by: "agent",
  additions: 1,
  deletions: 1,
  blocked: false,
  protected: false,
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
  can_undo_discard: false,
  can_undo_keep: false,
  can_publish_kept: false,
  fingerprint: null,
  git_changes: [],
  git_unchecked: false,
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
  /** Holds the next call of this method until the promise settles. */
  holdNext = new Map<Request["method"], Promise<void>>();
  /** Diffs say they were read with this fingerprint instead of the item's. */
  diffFingerprint: string | null = null;
  /** What `review.check_log` answers, by item. */
  checkLogs = new Map<string, string[]>();
  /** What `review.warnings` answers, by item; none by default. */
  warnings = new Map<string, ReviewWarning[]>();
  /** What `pr.prepare` says of the person's `gh`. */
  gh: GhStatus = { installed: true, version: "2.60.0", host: "github.com", login: "octo", signed_in: true };
  /** Files `pr.prepare` says conflict with the base. */
  prConflicts: string[] = [];
  /** What `pr.refresh` finds. */
  prStatus: PrStatus | null = null;
  /** What `agents.transcript` answers, by session. */
  transcripts = new Map<string, AgentMessage[]>();
  /**
   * The agent's answer to the next `agents.start`: an assistant message and the turn's
   * end right after the start answers. Null: the agent stays at work.
   */
  agentAnswer: string | null = null;
  private sessions = 0;
  /** What `agents.list` answers. */
  listed: SessionInfo[] = [];
  /** What `orchestration.list` answers. */
  tasks: Task[] = [];
  private listeners = new Set<(event: CoreEvent) => void>();

  async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    this.calls.push(request);
    const failure = this.failNext.get(request.method);
    if (failure) {
      this.failNext.delete(request.method);
      throw failure;
    }
    const hold = this.holdNext.get(request.method);
    if (hold) {
      this.holdNext.delete(request.method);
      await hold;
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

  /** What `projects.list` answers. */
  projects: Project[] = [{ id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false }];

  private find(id: string): ReviewItem {
    const item = this.items.find((i) => i.id === id);
    if (!item) throw new CoreError({ code: "not_found", message: `no review ${id}` });
    return item;
  }

  /** Like the core: protected files are published only when included. */
  private publishable(item: ReviewItem, included: boolean): void {
    if (!included && item.files.some((f) => f.protected))
      throw new CoreError({ code: "bad_request", message: "these changes include protected files" });
  }

  private answer(request: Request): unknown {
    switch (request.method) {
      case "review.list":
        // Copies, as over the wire: later changes here must not reach the UI's items.
        return {
          type: "review.list",
          // A task across repositories lists its items in each of them.
          data: structuredClone(
            this.items.filter(
              (i) => i.project === request.params.project || i.task_repos?.includes(request.params.project),
            ),
          ),
        };
      case "review.diff": {
        const item = this.find(request.params.id);
        const gone = this.diffGone.get(request.params.path);
        if (gone) throw new CoreError({ code: "not_found", message: gone });
        const fingerprint = this.diffFingerprint ?? item.fingerprint;
        return {
          type: "review.diff",
          data: { path: request.params.path, patch: patchFor(request.params.path), fingerprint },
        };
      }
      case "review.approve":
      case "review.reject":
      case "review.undo_reject": {
        const item = this.find(request.params.id);
        // Like the core: protected files land only when included.
        const guarded = item.files.filter((f) => f.protected && !f.blocked).map((f) => f.path);
        const included = request.method === "review.approve" && request.params.include_protected === true;
        if (request.method === "review.approve" && !included && guarded.length > 0) {
          if (item.mode === "direct")
            throw new CoreError({ code: "bad_request", message: "these changes include protected files" });
          const left = [...new Set([...this.conflicts, ...guarded])].sort();
          item.state = "partly_approved";
          item.not_copied = left;
          return { type: "review.done", data: { item: structuredClone(item), conflicts: left, restored: [] } };
        }
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
        // Like the core: only a keep without a commit, and only the latest, can be undone.
        if (request.method === "review.approve") for (const other of this.items) other.can_undo_keep = false;
        item.can_undo_keep = request.method === "review.approve" && request.params.commit === null;
        item.can_publish_kept = request.method === "review.approve" && item.mode === "isolated";
        // Like the core: a decided item has no fingerprint.
        if (request.method === "review.approve") item.fingerprint = null;
        if (item.state !== "approved") item.can_publish_kept = false;
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
        if (item.not_copied.length > 0) item.state = "partly_approved";
        item.can_undo_take = false;
        return {
          type: "review.done",
          data: { item: structuredClone(item), conflicts: [...this.conflicts], restored: [...this.restored] },
        };
      }
      case "review.update": {
        // Like the core: conflicting files are written only when asked; then the item is no longer behind.
        const item = this.find(request.params.id);
        const clashes = item.behind?.clashes ?? [];
        const written = clashes.length === 0 || request.params.conflicts === true;
        if (written) delete item.behind;
        const data = { item: structuredClone(item), files: written ? 2 : 0, conflicts: [...clashes], written };
        return { type: "review.updated", data };
      }
      case "review.undo_keep": {
        const item = this.find(request.params.id);
        if (!item.can_undo_keep) throw new CoreError({ code: "conflict", message: "there is no keep to undo" });
        item.state = "open";
        item.not_copied = [];
        item.can_undo_keep = false;
        return {
          type: "review.done",
          data: { item: structuredClone(item), conflicts: [...this.conflicts], restored: [...this.restored] },
        };
      }
      case "review.resolve": {
        const item = this.find(request.params.id);
        item.state = "approved";
        item.can_undo_take = false;
        return { type: "review.done", data: { item: structuredClone(item), conflicts: [], restored: [] } };
      }
      case "review.discard_hunk":
      case "review.undo_discard": {
        const item = this.find(request.params.id);
        item.can_undo_discard = request.method === "review.discard_hunk";
        const path = request.method === "review.discard_hunk" ? request.params.path : (item.files[0]?.path ?? "");
        return { type: "review.done", data: { item: structuredClone(item), conflicts: [], restored: [path] } };
      }
      case "review.comment":
        this.find(request.params.id);
        return { type: "ok" };
      case "review.warnings":
        this.find(request.params.id);
        return { type: "review.warnings", data: { warnings: this.warnings.get(request.params.id) ?? [] } };
      case "review.run_check":
        this.find(request.params.id);
        return { type: "ok" };
      case "review.check_log": {
        const lines = this.checkLogs.get(request.params.id);
        if (!lines) throw new CoreError({ code: "not_found", message: "a check of this review not found" });
        return { type: "review.check_log", data: { lines } };
      }
      case "pr.prepare": {
        const item = this.find(request.params.id);
        const data = {
          remote: "origin",
          host: this.gh.host,
          base: request.params.base ?? "main",
          branch: "ovra/t1/docs",
          title: "Write the notes",
          body: item.summary ?? "",
          files: structuredClone(item.files),
          conflicts: [...this.prConflicts],
          // A kept item lists none; the core reads the kept files' then.
          fingerprint: item.fingerprint ?? "kept",
          gh: { ...this.gh },
        };
        return { type: "pr.prepared", data };
      }
      case "pr.create": {
        const item = this.find(request.params.id);
        this.publishable(item, request.params.include_protected === true);
        const { remote, base, branch } = request.params.draft;
        const url = "https://github.com/o/r/pull/7";
        // A kept item stays kept.
        if (item.state === "open") item.state = "published";
        item.can_publish_kept = false;
        item.can_undo_keep = false;
        item.pull_request = { number: 7, url, remote, base, branch, pushed: "c1", status: null };
        return { type: "pr.pull_request", data: structuredClone(item.pull_request) };
      }
      case "pr.update":
      case "pr.refresh": {
        const item = this.find(request.params.id);
        const pr = item.pull_request;
        if (!pr) throw new CoreError({ code: "conflict", message: "this item has no pull request" });
        if (request.method === "pr.update") {
          this.publishable(item, request.params.include_protected === true);
          pr.pushed = "c2";
          item.state = "published";
        } else pr.status = structuredClone(this.prStatus);
        return { type: "pr.pull_request", data: structuredClone(pr) };
      }
      case "pr.send_log":
      case "pr.ask_update":
        this.find(request.params.id);
        return { type: "ok" };
      case "open.url":
        return { type: "ok" };
      case "agents.start": {
        const session = `agent${++this.sessions}`;
        const answer = this.agentAnswer;
        if (answer !== null)
          // Before the caller sees the answer: messages can come first.
          queueMicrotask(() => {
            this.emit({
              type: "agents.message",
              data: { session, message: { kind: "assistant_text", text: answer }, seq: 1 },
            });
            const usage = { input_tokens: 0, output_tokens: 0, cost_micro_usd: null };
            this.emit({ type: "agents.message", data: { session, message: { kind: "turn_end", usage }, seq: 2 } });
          });
        return { type: "agents.session", data: { id: session } };
      }
      case "agents.stop":
        return { type: "ok" };
      case "agents.list":
        return { type: "agents.list", data: structuredClone(this.listed) };
      case "orchestration.list":
        return { type: "orchestration.list", data: structuredClone(this.tasks) };
      case "projects.list":
        return { type: "projects.list", data: structuredClone(this.projects) };
      case "agents.transcript": {
        const messages = this.transcripts.get(request.params.session) ?? [];
        return { type: "agents.transcript", data: { messages, seq: messages.length } };
      }
      // A project without AGENTS.md, for Remember this.
      case "fs.read_file":
        throw new CoreError({ code: "not_found", message: "no such file" });
      case "fs.write_file":
        return { type: "fs.written", data: { mtime_ms: 1 } };
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }
}
