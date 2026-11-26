// Test-only fake of the core's agents and permissions: records calls, answers
// them like the core and lets a test push events.

import { claudeCall } from "../approvals/claude-calls";
import type {
  AccountState,
  AccountsOverview,
  AgentCapacity,
  AgentKind,
  AgentMessage,
  AgentSetup,
  AgentStatus,
  BoardEntry,
  BranchPr,
  Checkpoint,
  ContextWarning,
  CoreEvent,
  Issue,
  Leftover,
  PermissionRequest,
  Project,
  ProjectAgentConfig,
  ProjectId,
  ProviderInfo,
  Request,
  ResponseFor,
  ReviewItem,
  SandboxReport,
  SessionId,
  SessionInfo,
  SetupReport,
  Task,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

const mapped = (message: AgentMessage): AgentMessage =>
  message.kind === "tool_use" && !message.call
    ? { ...message, call: claudeCall(message.name, message.input) }
    : message;

export class FakeAgentCore implements Transport {
  calls: Request[] = [];
  sessions: SessionInfo[] = [];
  transcripts = new Map<SessionId, AgentMessage[]>();
  pending: PermissionRequest[] = [];
  /** Tasks `orchestration.start` made; a test adds their workers. */
  tasks: Task[] = [];
  /** What `orchestration.leftovers` answers; `orchestration.remove_leftover` takes one out. */
  leftovers: Leftover[] = [];
  /** Each task's board; `board.post` and `board.answer` add the person's entries. */
  boards = new Map<string, BoardEntry[]>();
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
  /** What `setup.check` answers; `null`: it fails. */
  setupReport: SetupReport | null = null;
  /** What `agents.setup` answers for a provider other than Claude, when set. */
  setups: Partial<Record<AgentKind, AgentSetup>> = {};
  /** What `agents.providers` answers. */
  providers: ProviderInfo[] = [
    {
      id: "claude",
      name: "Claude",
      capabilities: {
        approvals: true,
        enforced_policy: true,
        ide_tools: true,
        read_only_role: true,
        resume: true,
        fork: true,
        interrupt: true,
        text_deltas: true,
        subagents: true,
        usage: true,
        cost: true,
        billing: true,
        accounts: true,
        images: true,
      },
    },
  ];
  /** The accounts `agents.accounts` reports; the default one first. */
  accounts: AccountState[] = [
    {
      account: { provider: "claude", id: "default", name: "Default", config_dir: null },
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
  capacity: AgentCapacity = { working: 0, limit: 3, queued: 0, research_working: 0, research_limit: 2, waiting: [] };
  /** Files `fs.create` made and `fs.write_file` wrote, by path. */
  files = new Map<string, string>();
  /** Sessions `agents.stop` was called for, in order. */
  stopped: SessionId[] = [];
  /** `git.status` answers that the project is not a git repository. */
  noRepo = false;
  /** The project's files, for the @ menu (`fs.list_files`). */
  listed: string[] = [];
  /** The repository's open issues (`pr.issues`, `pr.issue`). */
  issues: Issue[] = [];
  /** What `pr.branch_checks` answers; `pr.check_log` reads its failed runs' logs from `logs`. */
  branchPr: BranchPr | null = null;
  logs = new Map<number, string>();
  /** What `context.check` answers for an item's path or text. */
  contextNotes = new Map<string, ContextWarning>();
  /** What `agents.checkpoints` answers. */
  checkpoints: Checkpoint[] = [];
  /** What `agents.go_back` and `agents.undo_go_back` report. */
  wentBack: { restored: string[]; conflicts: string[] } = { restored: [], conflicts: [] };
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

  /** A tool call without `call` gets it as the Claude runtime maps it, as from a real core. */
  message(session: SessionId, given: AgentMessage): void {
    const message = mapped(given);
    const transcript = [...(this.transcripts.get(session) ?? []), message];
    this.transcripts.set(session, transcript);
    this.emit({ type: "agents.message", data: { session, message, seq: transcript.length } });
  }

  /** The runtime says how it runs the session (`null`: without a sandbox). */
  sandboxed(session: SessionId, sandbox: SandboxReport | null): void {
    const info = this.sessions.find((s) => s.id === session);
    if (info) Object.assign(info, { sandbox, sandbox_reported: true });
    this.emit({ type: "agents.sandbox", data: { session, sandbox } });
  }

  /** A request without `call` gets it as the Claude runtime maps it. */
  ask(given: PermissionRequest): void {
    const request = { ...given, call: given.call ?? claudeCall(given.tool, given.input) };
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
        if (request.params.solo) {
          info.agent = request.params.agent ?? "claude";
          info.mode = "isolated";
          const id = `t${this.tasks.length + 1}`;
          const task: Task = {
            id,
            project,
            coordinator: null,
            workers: [
              { session: info.id, name: "agent", branch: `ovra/${id}/agent`, worktree: `/wt/${id}/agent`, kept: null },
            ],
            limits: { max_workers: 1, budget_micro_usd: null },
            spent_micro_usd: 0,
            ...(request.params.attempt_of ? { attempt_of: request.params.attempt_of } : {}),
          };
          this.tasks.push(task);
          return { type: "orchestration.task", data: { ...task } };
        }
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
      case "board.list": {
        const entries = this.boards.get(request.params.task) ?? [];
        return {
          type: "board.entries",
          data: { entries: entries.slice(request.params.since ?? 0), next: entries.length, open_for_person: [] },
        };
      }
      case "board.post":
      case "board.answer": {
        const entries = this.boards.get(request.params.task) ?? [];
        const { params } = request;
        const entry: BoardEntry = {
          id: `b${entries.length + 1}`,
          author: { kind: "person" },
          body:
            "question" in params
              ? { kind: "answer", question: params.question, text: params.text }
              : { kind: "note", text: params.text },
        };
        this.boards.set(params.task, [...entries, entry]);
        return { type: "board.entry", data: entry };
      }
      case "agents.transcript": {
        const messages = (this.transcripts.get(request.params.session) ?? []).map(mapped);
        return { type: "agents.transcript", data: { messages, seq: messages.length } };
      }
      case "agents.capacity":
        return { type: "agents.capacity", data: { ...this.capacity } };
      case "agents.stop":
        this.stopped.push(request.params.session);
        return { type: "ok" };
      case "agents.checkpoints":
        return { type: "agents.checkpoints", data: this.checkpoints };
      case "agents.go_back":
        return { type: "agents.went_back", data: { ...this.wentBack, can_undo: this.wentBack.restored.length > 0 } };
      case "agents.undo_go_back":
        return { type: "agents.undid_go_back", data: this.wentBack };
      case "agents.send":
      case "browser.attach":
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
      case "setup.check":
        if (!this.setupReport) throw new CoreError({ code: "unavailable", message: "setup.check is not faked" });
        return { type: "setup.report", data: this.setupReport };
      case "agents.providers":
        return { type: "agents.providers", data: this.providers.map((p) => ({ ...p })) };
      case "agents.setup":
        return { type: "agents.setup", data: { ...(this.setups[request.params.agent] ?? this.setup) } };
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
      case "fs.list_dir":
        throw new CoreError({ code: "not_found", message: "not a folder" });
      case "context.attach":
        return { type: "context.attached", data: { path: `.ovra-attachments/${request.params.name}` } };
      case "fs.read_file": {
        // Pasted PNGs read back as small images; other files are not there.
        const path = request.params.path;
        if (!path.startsWith(".ovra-attachments/") || !path.endsWith(".png"))
          throw new CoreError({ code: "not_found", message: `${path} not found` });
        const body = { kind: "binary" as const, mime: "image/png", base64: "iVBO" };
        return { type: "fs.file", data: { path, size: 3, mtime_ms: 1, body } };
      }
      case "fs.list_files":
        return { type: "fs.files", data: { paths: [...this.listed], total: this.listed.length, version: 1 } };
      case "context.check":
        return {
          type: "context.checked",
          data: {
            notes: request.params.items.map((item) => {
              const key = "path" in item ? item.path : "text" in item ? item.text : "";
              return this.contextNotes.get(key) ?? null;
            }),
          },
        };
      case "pr.issues":
        return { type: "pr.issues", data: this.issues.map(({ number, title, url }) => ({ number, title, url })) };
      case "pr.issue": {
        const issue = this.issues.find((i) => i.number === request.params.number);
        if (!issue) throw new CoreError({ code: "not_found", message: `issue ${request.params.number}` });
        return { type: "pr.issue", data: issue };
      }
      case "pr.branch_checks":
        return { type: "pr.branch_checks", data: this.branchPr };
      case "pr.check_log": {
        const check = this.branchPr?.failed.find((c) => c.run === request.params.run);
        if (!check || !this.branchPr) throw new CoreError({ code: "not_found", message: "no such check" });
        const text = this.logs.get(request.params.run) ?? "";
        return { type: "pr.check_log", data: { pr: this.branchPr.number, name: check.name, url: check.url, text } };
      }
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
      case "orchestration.leftovers":
        return { type: "orchestration.leftovers", data: [...this.leftovers] };
      case "orchestration.remove_leftover":
        this.leftovers = this.leftovers.filter((l) => l.path !== request.params.path);
        return { type: "ok" };
      case "orchestration.retry_setup":
        return { type: "ok" };
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
