// Test-only fake of the core's agents and permissions: records calls, answers
// them like the core and lets a test push events.

import type {
  AccountState,
  AccountsOverview,
  AgentCapacity,
  AgentMessage,
  AgentSetup,
  AgentStatus,
  CoreEvent,
  PermissionRequest,
  Project,
  ProjectAgentConfig,
  ProjectId,
  Request,
  ResponseFor,
  ReviewItem,
  SandboxReport,
  SessionId,
  SessionInfo,
  Task,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class FakeAgentCore implements Transport {
  calls: Request[] = [];
  sessions: SessionInfo[] = [];
  transcripts = new Map<SessionId, AgentMessage[]>();
  pending: PermissionRequest[] = [];
  /** Tasks `orchestration.start` made; a test adds their workers. */
  tasks: Task[] = [];
  /** What `review.list` answers, newest first. */
  reviews: ReviewItem[] = [];
  /** What `projects.list` answers. */
  projects: Project[] = [{ id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false }];
  /** What `agents.setup` answers; `agents.set_path` records the path in it. */
  setup: AgentSetup = {
    path: null,
    executable: "/usr/bin/claude",
    version: "2.1.331",
    state: { state: "ready" },
    billing: { kind: "subscription" },
    api_key_in_env: false,
  };
  /** The accounts `agents.accounts` reports; the default one first. */
  accounts: AccountState[] = [
    {
      account: { id: "default", name: "Default", config_dir: null },
      setup: { ...this.setup },
    },
  ];
  /** Folders `agents.accounts` reports as found. */
  found: string[] = [];
  projectAccounts = new Map<ProjectId, string>();
  /** What `agents.project_config` reports as current. */
  config: ProjectAgentConfig = { fingerprint: "f0", items: [] };
  /** What the user trusted, per project. */
  trustedConfig = new Map<ProjectId, ProjectAgentConfig>();
  /** What `agents.capacity` answers. */
  capacity: AgentCapacity = { working: 0, limit: 3, queued: 0, research_working: 0, research_limit: 2 };
  /** Files `fs.create` made and `fs.write_file` wrote, by path. */
  files = new Map<string, string>();
  /** `git.status` answers that the project is not a git repository. */
  noRepo = false;
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

  /** Calls of one method, e.g. every `permissions.answer`. */
  sent<M extends Request["method"]>(method: M): Extract<Request, { method: M }>[] {
    return this.calls.filter((c): c is Extract<Request, { method: M }> => c.method === method);
  }

  status(session: SessionId, status: AgentStatus): void {
    const info = this.sessions.find((s) => s.id === session);
    if (info) info.status = status;
    this.emit({ type: "agents.status", data: { session, project: info?.project ?? "p1", status } });
  }

  message(session: SessionId, message: AgentMessage): void {
    this.transcripts.set(session, [...(this.transcripts.get(session) ?? []), message]);
    this.emit({ type: "agents.message", data: { session, message } });
  }

  /** The runtime says how it runs the session (`null`: without a sandbox). */
  sandboxed(session: SessionId, sandbox: SandboxReport | null): void {
    const info = this.sessions.find((s) => s.id === session);
    if (info) Object.assign(info, { sandbox, sandbox_reported: true });
    this.emit({ type: "agents.sandbox", data: { session, sandbox } });
  }

  ask(request: PermissionRequest): void {
    this.pending.push(request);
    this.emit({ type: "permissions.requested", data: request });
  }

  private answer(request: Request): unknown {
    switch (request.method) {
      case "agents.list":
        return { type: "agents.list", data: this.sessions.filter((s) => s.project === request.params.project) };
      case "agents.start": {
        const { project, prompt } = request.params;
        const info = this.addSession(project, prompt.split("\n")[0] ?? "");
        info.account = request.params.account ?? null;
        info.mode = request.params.mode;
        this.status(info.id, { state: "starting" });
        this.message(info.id, { kind: "user_text", text: prompt, from: "person" });
        return { type: "agents.session", data: { ...info } };
      }
      case "orchestration.start": {
        const { project, prompt } = request.params;
        const info = this.addSession(project, prompt.split("\n")[0] ?? "");
        this.status(info.id, { state: "starting" });
        const task: Task = {
          id: `t${this.tasks.length + 1}`,
          project,
          coordinator: info.id,
          workers: [],
          limits: { max_workers: 3, budget_micro_usd: null },
          spent_micro_usd: 0,
        };
        this.tasks.push(task);
        return { type: "orchestration.task", data: { ...task } };
      }
      case "orchestration.list":
        return { type: "orchestration.list", data: this.tasks.filter((t) => t.project === request.params.project) };
      case "agents.transcript":
        return { type: "agents.transcript", data: this.transcripts.get(request.params.session) ?? [] };
      case "agents.capacity":
        return { type: "agents.capacity", data: { ...this.capacity } };
      case "agents.send":
      case "browser.attach":
      case "agents.stop":
      case "agents.interrupt":
        return { type: "ok" };
      case "agents.resume": {
        const info = this.sessions.find((s) => s.id === request.params.session);
        if (!info) throw new CoreError({ code: "not_found", message: "no such session" });
        // Like the core, a restart forgets how the last process ran.
        Object.assign(info, { resumable: false, sandbox: null, sandbox_reported: false });
        this.status(info.id, { state: "starting" });
        this.message(info.id, { kind: "user_text", text: request.params.prompt, from: "person" });
        return { type: "agents.session", data: { ...info } };
      }
      case "agents.setup":
        return { type: "agents.setup", data: { ...this.setup } };
      case "agents.set_path":
        this.setup = { ...this.setup, path: request.params.path };
        return { type: "agents.setup", data: { ...this.setup } };
      case "agents.accounts":
        return { type: "agents.accounts", data: this.accountsOverview(request.params.project) };
      case "agents.save_account": {
        const account = { ...request.params.account };
        if (!account.id) account.id = `account-${this.accounts.length}`;
        this.accounts = [...this.accounts.filter((a) => a.account.id !== account.id), { account, setup: this.setup }];
        this.found = this.found.filter((dir) => dir !== account.config_dir);
        return { type: "agents.accounts", data: this.accountsOverview(null) };
      }
      case "agents.remove_account":
        this.accounts = this.accounts.filter((a) => a.account.id !== request.params.id);
        return { type: "agents.accounts", data: this.accountsOverview(null) };
      case "agents.set_project_account":
        if (request.params.account) this.projectAccounts.set(request.params.project, request.params.account);
        else this.projectAccounts.delete(request.params.project);
        return { type: "ok" };
      case "agents.project_config":
        return {
          type: "agents.project_config",
          data: { current: this.config, trusted: this.trustedConfig.get(request.params.project) ?? null },
        };
      case "projects.list":
        return { type: "projects.list", data: [...this.projects] };
      case "projects.trust": {
        const { id, trusted, fingerprint } = request.params;
        if (trusted && fingerprint !== this.config.fingerprint)
          throw new CoreError({ code: "conflict", message: "the project's settings changed; review them again" });
        if (trusted) this.trustedConfig.set(id, this.config);
        else this.trustedConfig.delete(id);
        return { type: "projects.project", data: { id, name: "demo", root: "/demo", last_opened_ms: null, trusted } };
      }
      case "open.url":
        return { type: "ok" };
      case "fs.create":
        if (this.files.has(request.params.path))
          throw new CoreError({ code: "conflict", message: `${request.params.path} exists` });
        this.files.set(request.params.path, "");
        return { type: "ok" };
      case "fs.write_file":
        this.files.set(request.params.path, request.params.text);
        return { type: "fs.written", data: { mtime_ms: 1 } };
      case "git.status":
        if (this.noRepo) return { type: "git.no_repo" };
        return {
          type: "git.status",
          data: {
            branch: "main",
            head: null,
            upstream: null,
            ahead: 0,
            behind: 0,
            operation: null,
            files: [],
            truncated: 0,
          },
        };
      case "review.list":
        return { type: "review.list", data: this.reviews.filter((r) => r.project === request.params.project) };
      case "permissions.pending":
        return { type: "permissions.pending", data: [...this.pending] };
      case "permissions.answer": {
        const { id, decision } = request.params;
        if (!this.pending.some((r) => r.id === id))
          throw new CoreError({ code: "not_found", message: `${id} is not pending` });
        this.pending = this.pending.filter((r) => r.id !== id);
        this.emit({ type: "permissions.resolved", data: { id, decision } });
        return { type: "ok" };
      }
      default:
        throw new CoreError({ code: "unavailable", message: `${request.method} is not faked` });
    }
  }

  private accountsOverview(project: ProjectId | null): AccountsOverview {
    return {
      accounts: this.accounts.map((a) => ({ ...a })),
      found: [...this.found],
      project_account: project ? (this.projectAccounts.get(project) ?? null) : null,
    };
  }

  addSession(project: ProjectId, title: string, status: AgentStatus = { state: "starting" }): SessionInfo {
    const info: SessionInfo = {
      id: `s${this.sessions.length + 1}`,
      project,
      agent: "claude",
      mode: "direct",
      status,
      title,
      usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
      resumable: false,
      billing: null,
      sandbox: null,
      sandbox_reported: false,
      account: null,
    };
    this.sessions.push(info);
    return info;
  }
}
