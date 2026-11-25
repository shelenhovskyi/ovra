// Test-only fake of the core's git requests: a small repository in memory
// that answers like the core, records calls and lets a test push events.

import type {
  BranchInfo,
  CommitInfo,
  CoreEvent,
  FileDiff,
  RepoStatus,
  Request,
  ResponseFor,
  StatusFile,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export const changed = (path: string, extra: Partial<StatusFile> = {}): StatusFile => ({
  path,
  from: null,
  staged: null,
  unstaged: "modified",
  ...extra,
});

const commitInfo = (id: string, summary: string): CommitInfo => ({ id, author: "Ada", time: 0, summary });

export const repo = (extra: Partial<RepoStatus> = {}): RepoStatus => ({
  branch: "main",
  head: "abc1234def",
  upstream: null,
  ahead: 0,
  behind: 0,
  operation: null,
  files: [],
  truncated: 0,
  ...extra,
});

export class FakeGitCore implements Transport {
  calls: Request[] = [];
  /** Null: the project is not in a repository. */
  status: RepoStatus | null = repo();
  branches: BranchInfo[] = [{ name: "main", current: true, upstream: null, commit: null }];
  commits: CommitInfo[] = [commitInfo("abc1234def", "First commit")];
  /** `git.diff` answers, by `path` or `staged:path`. */
  diffs = new Map<string, FileDiff>();
  /** Makes the next call of this method fail with this error. */
  failNext = new Map<Request["method"], CoreError>();
  private discards = new Map<string, StatusFile[]>();
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

  /** The methods called, in order. */
  methods(): string[] {
    return this.calls.map((call) => call.method);
  }

  private file(path: string): StatusFile | undefined {
    return this.status?.files.find((f) => f.path === path);
  }

  private answer(request: Request): unknown {
    const status = this.status;
    if (request.method === "git.status")
      return status ? { type: "git.status", data: structuredClone(status) } : { type: "git.no_repo" };
    if (!status) throw new CoreError({ code: "bad_request", message: "not a git repository" });
    switch (request.method) {
      case "git.stage":
        for (const path of request.params.paths) {
          const file = this.file(path);
          if (!file) continue;
          file.staged = file.unstaged === "untracked" ? "added" : (file.unstaged ?? file.staged);
          file.unstaged = null;
        }
        return { type: "ok" };
      case "git.unstage":
        for (const path of request.params.paths) {
          const file = this.file(path);
          if (!file) continue;
          file.unstaged = file.staged === "added" ? "untracked" : (file.unstaged ?? file.staged);
          file.staged = null;
        }
        status.files = status.files.filter((f) => f.staged || f.unstaged);
        return { type: "ok" };
      case "git.discard": {
        const undo = `undo-${this.discards.size + 1}`;
        this.discards.set(undo, structuredClone(status.files));
        for (const path of request.params.paths) {
          const file = this.file(path);
          if (file) file.unstaged = null;
        }
        status.files = status.files.filter((f) => f.staged || f.unstaged);
        return { type: "git.discarded", data: { undo } };
      }
      case "git.undo_discard": {
        const files = this.discards.get(request.params.undo);
        if (!files) throw new CoreError({ code: "not_found", message: "no such snapshot" });
        status.files = files;
        return { type: "ok" };
      }
      case "git.commit": {
        if (!request.params.amend && !status.files.some((f) => f.staged))
          throw new CoreError({ code: "bad_request", message: "nothing is staged" });
        const commit = commitInfo(`c${this.commits.length}00000`, request.params.message.split("\n")[0] ?? "");
        this.commits = [commit, ...(request.params.amend ? this.commits.slice(1) : this.commits)];
        for (const file of status.files) file.staged = null;
        status.files = status.files.filter((f) => f.unstaged);
        status.head = commit.id;
        return { type: "git.committed", data: { commit, output: "" } };
      }
      case "git.branches":
        return { type: "git.branches", data: { branches: structuredClone(this.branches) } };
      case "git.switch":
      case "git.create_branch": {
        const name = request.method === "git.switch" ? request.params.branch : request.params.name;
        if (request.method === "git.create_branch") {
          if (this.branches.some((b) => b.name === name))
            throw new CoreError({ code: "conflict", message: `a branch named ${name} already exists` });
          this.branches.push({ name, current: false, upstream: null, commit: null });
        }
        for (const branch of this.branches) branch.current = branch.name === name;
        status.branch = name;
        return { type: "ok" };
      }
      case "git.diff": {
        const key = request.params.staged ? `staged:${request.params.path}` : request.params.path;
        const empty = { path: request.params.path, from: null, binary: false, hunks: [], truncated: false };
        return { type: "git.diff", data: structuredClone(this.diffs.get(key) ?? empty) };
      }
      case "git.log":
        return { type: "git.log", data: { commits: this.commits.slice(0, request.params.limit ?? 50) } };
      default:
        return { type: "ok" };
    }
  }
}
