// Agent sessions the UI knows of, kept current from `agents.*` events. One
// instance per transport, shared by the agents panel, approvals and
// notifications (`agentSessions(transport)`).

import { SvelteMap } from "svelte/reactivity";
import { openInboxFor } from "../inbox/open";
import { listCommands } from "../keys/commands";
import type {
  AgentCapacity,
  AgentMessage,
  Billing,
  CoreEvent,
  FileChange,
  ProjectId,
  ReviewItem,
  SessionId,
  SessionInfo,
  TaskId,
  TurnUsage,
} from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { isBusy, statusLabel } from "./status";

/** A session's part in a task, if it has one. */
export type SessionRole =
  | { kind: "coordinator"; task: TaskId }
  | {
      kind: "worker";
      task: TaskId;
      name: string;
      branch: string;
      /** Why its worktree stays although its task ended. */
      kept: string | null;
      coordinator: SessionId;
    };

export class AgentSessions {
  /** Coordinators and workers of the projects' tasks. */
  readonly roles = new SvelteMap<SessionId, SessionRole>();
  /** Every known session by id, in the order they became known. */
  readonly byId = new SvelteMap<SessionId, SessionInfo>();
  /** Transcripts of sessions watched since they started or since `loadTranscript`. */
  readonly transcripts = new SvelteMap<SessionId, AgentMessage[]>();
  /** Each session's newest review item: what its agent changed, kept or not. */
  readonly reviews = new SvelteMap<SessionId, ReviewItem>();
  /** How many of each loaded project's agents work, of how many may. */
  readonly capacity = new SvelteMap<ProjectId, AgentCapacity>();
  private readonly loaded = new Set<ProjectId>();
  /** The latest `agents.capacity` call per project: older replies are stale. */
  private readonly asked = new Map<ProjectId, number>();

  constructor(private readonly transport: Transport) {
    transport.onEvent((event) => this.apply(event));
    let wasOpen = false;
    transport.onStatus?.((status) => {
      if (status !== "open") return;
      // After a reconnect events may have been missed: ask again.
      if (wasOpen) this.resync();
      wasOpen = true;
    });
  }

  /** Sessions of `project`, oldest first. */
  inProject(project: ProjectId): SessionInfo[] {
    return [...this.byId.values()].filter((s) => s.project === project);
  }

  async load(project: ProjectId): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.list", params: { project } });
    this.loaded.add(project);
    for (const info of data) this.byId.set(info.id, info);
    await Promise.all([this.loadTasks(project), this.loadReviews(project), this.loadCapacity(project)]);
  }

  /** Each session's newest review item. Front ends without reviews have none. */
  async loadReviews(project: ProjectId): Promise<void> {
    let items: ReviewItem[];
    try {
      ({ data: items } = await this.transport.call({ method: "review.list", params: { project } }));
    } catch {
      return;
    }
    for (const [session, item] of this.reviews) if (item.project === project) this.reviews.delete(session);
    // Newest first: the first item of a session is its current one.
    for (const item of items) if (!this.reviews.has(item.session)) this.reviews.set(item.session, item);
  }

  /** The files `id`'s agent changed that wait for the person's review. */
  changes(id: SessionId): FileChange[] {
    const item = this.reviews.get(id);
    return item && waits(item) ? item.files.filter((f) => f.by !== "outside") : [];
  }

  /** How many of the task's workers have changes waiting for review. */
  waitingInTask(coordinator: SessionId): number {
    return [...this.roles].filter(([session, role]) => {
      const item = this.reviews.get(session);
      return role.kind === "worker" && role.coordinator === coordinator && item !== undefined && waits(item);
    }).length;
  }

  /** The task's workers, in the order it started them. */
  workers(coordinator: SessionId): SessionId[] {
    return [...this.roles]
      .filter(([, role]) => role.kind === "worker" && role.coordinator === coordinator)
      .map(([session]) => session);
  }

  /** Front ends without a count show none. */
  private async loadCapacity(project: ProjectId): Promise<void> {
    const call = (this.asked.get(project) ?? 0) + 1;
    this.asked.set(project, call);
    try {
      const { data } = await this.transport.call({ method: "agents.capacity", params: { project } });
      if (this.asked.get(project) === call) this.capacity.set(project, data);
    } catch {
      // No count to show.
    }
  }

  /** Who coordinates and who works in the project's tasks. Front ends without tasks have none. */
  private async loadTasks(project: ProjectId): Promise<void> {
    try {
      const { data } = await this.transport.call({ method: "orchestration.list", params: { project } });
      for (const task of data) {
        this.roles.set(task.coordinator, { kind: "coordinator", task: task.id });
        for (const w of task.workers)
          this.roles.set(w.session, {
            kind: "worker",
            task: task.id,
            name: w.name,
            branch: w.branch,
            kept: w.kept,
            coordinator: task.coordinator,
          });
      }
    } catch {
      // No tasks to show.
    }
  }

  /**
   * A session's name for people: coordinators as the task, workers by
   * their name and their task's (`docs · Fix login`), so the workers of two
   * tasks differ.
   */
  name(id: SessionId): string {
    const role = this.roles.get(id);
    const base = sessionName(this.byId.get(id), id);
    if (role?.kind === "worker") {
      const task = this.byId.get(role.coordinator)?.title;
      return task ? `${role.name} · ${task}` : `Worker ${role.name}`;
    }
    if (role?.kind === "coordinator") return `Task: ${base}`;
    return base;
  }

  /**
   * Resolves once `name(id)` says what the core knows: a session that just
   * started may not have its title, or a worker its task, yet. A message
   * built now then names it like the Tasks list does.
   */
  async named(project: ProjectId, id: SessionId): Promise<void> {
    const info = this.byId.get(id);
    if (info?.title && (info.mode === "direct" || this.roles.has(id))) return;
    await this.load(project).catch(() => {});
  }

  /**
   * What `id` does now, in words; a coordinator that waits while its
   * workers run, or wait for a free agent, says so.
   */
  status(id: SessionId): string {
    const info = this.byId.get(id);
    if (!info) return "";
    const role = this.roles.get(id);
    if (role?.kind === "coordinator" && info.status.state === "idle") {
      const workers = [...this.roles]
        .filter(([, r]) => r.kind === "worker" && r.coordinator === id)
        .map(([session]) => this.byId.get(session)?.status ?? null);
      if (workers.some(isBusy)) return "Workers are on it";
      if (workers.some((s) => s?.state === "queued")) return "Workers are waiting for a free agent";
    }
    return statusLabel(info.status);
  }

  /** Sessions of `project` with each task's workers right after their coordinator. */
  grouped(project: ProjectId): SessionInfo[] {
    const all = this.inProject(project);
    const workersOf = (coordinator: SessionId) =>
      all.filter((s) => {
        const role = this.roles.get(s.id);
        return role?.kind === "worker" && role.coordinator === coordinator;
      });
    return all
      .filter((s) => this.roles.get(s.id)?.kind !== "worker")
      .flatMap((s) => [s, ...workersOf(s.id)])
      .concat(
        all.filter((s) => {
          const role = this.roles.get(s.id);
          return role?.kind === "worker" && !all.some((c) => c.id === role.coordinator);
        }),
      );
  }

  /** Starts a task: a coordinator that splits the work among workers in
   * their own worktrees. Resolves with the coordinator's session. */
  async startTask(project: ProjectId, prompt: string): Promise<SessionId> {
    const { data } = await this.transport.call({ method: "orchestration.start", params: { project, prompt } });
    await this.load(project);
    return data.coordinator;
  }

  async loadTranscript(session: SessionId): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.transcript", params: { session } });
    this.transcripts.set(session, data);
  }

  /** `account`: a Claude account's id; null uses the project's. */
  async start(project: ProjectId, prompt: string, account: string | null = null): Promise<SessionInfo> {
    // Messages may arrive before the reply: collect them from the start.
    const { data } = await this.transport.call({
      method: "agents.start",
      params: { project, agent: "claude", mode: "direct", prompt, account },
    });
    const known = this.byId.get(data.id);
    // agents.sandbox may also have come before the reply: keep what it said.
    const sandbox = known?.sandbox_reported ? { sandbox: known.sandbox, sandbox_reported: true } : {};
    this.byId.set(data.id, { ...data, status: known?.status ?? data.status, ...sandbox });
    return data;
  }

  async send(session: SessionId, text: string): Promise<void> {
    await this.transport.call({ method: "agents.send", params: { session, text } });
  }

  /** Ends the current turn; the agent's pending requests are denied and it waits for a message. */
  async interrupt(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.interrupt", params: { session } });
  }

  /** Continues an ended session with a new prompt; it keeps its id and transcript. */
  async resume(session: SessionId, prompt: string): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.resume", params: { session, prompt } });
    this.byId.set(data.id, { ...data, status: this.byId.get(data.id)?.status ?? data.status });
  }

  async stop(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.stop", params: { session } });
  }

  private apply(event: CoreEvent): void {
    // Events were dropped: what is on screen may be missing or doubled.
    if (event.type === "info.resync") this.resync();
    else if (event.type === "orchestration.changed") void this.loadTasks(event.data.project);
    else if (event.type === "review.changed") {
      if (this.loaded.has(event.data.project)) void this.loadReviews(event.data.project);
    } else if (event.type === "agents.status") {
      const { session, project, status } = event.data;
      // Any change of state may change how many work.
      if (this.loaded.has(project)) void this.loadCapacity(project);
      const known = this.byId.get(session);
      // A turn ended: what it changed is in the session's review item now.
      if (known && status.state === "idle") void this.loadReviews(project);
      if (known) {
        // A restart clears the sandbox report without an event of its own.
        const fresh = status.state === "starting" ? { sandbox: null, sandbox_reported: false } : {};
        this.byId.set(session, { ...known, status, ...fresh });
        // Whether it can be resumed comes only with the list.
        if (status.state === "stopped" || status.state === "failed") void this.load(project).catch(() => {});
        return;
      }
      if (status.state === "starting") this.transcripts.set(session, []);
      // Its title comes with the list (or the reply to agents.start).
      this.byId.set(session, {
        id: session,
        project,
        agent: "claude",
        mode: "direct",
        status,
        title: "",
        usage: NO_USAGE,
        resumable: false,
        billing: null,
        sandbox: null,
        sandbox_reported: false,
        account: null,
      });
      void this.load(project).catch(() => {});
    } else if (event.type === "agents.billing") {
      const known = this.byId.get(event.data.session);
      if (known) this.byId.set(event.data.session, { ...known, billing: event.data.billing });
    } else if (event.type === "agents.sandbox") {
      const known = this.byId.get(event.data.session);
      if (known) this.byId.set(event.data.session, { ...known, sandbox: event.data.sandbox, sandbox_reported: true });
    } else if (event.type === "agents.message") {
      const { session, message } = event.data;
      const transcript = this.transcripts.get(session);
      if (transcript) this.transcripts.set(session, [...transcript, message]);
      const known = this.byId.get(session);
      if (known && message.kind === "turn_end")
        this.byId.set(session, { ...known, usage: add(known.usage, message.usage) });
    }
  }

  private resync(): void {
    this.transcripts.clear();
    for (const project of this.loaded) void this.load(project).catch(() => {});
  }
}

const NO_USAGE: TurnUsage = { input_tokens: 0, output_tokens: 0, cost_micro_usd: null };

function add(a: TurnUsage, b: TurnUsage): TurnUsage {
  const cost =
    a.cost_micro_usd === null && b.cost_micro_usd === null ? null : (a.cost_micro_usd ?? 0) + (b.cost_micro_usd ?? 0);
  return {
    input_tokens: a.input_tokens + b.input_tokens,
    output_tokens: a.output_tokens + b.output_tokens,
    cost_micro_usd: cost,
  };
}

/**
 * `12,345 tokens in, 678 out, $0.0123`. On a Claude plan the agent's cost is
 * only what the tokens would cost at API prices, so it says so.
 */
export function usageText(usage: TurnUsage, billing: Billing | null): string {
  const dollars = usage.cost_micro_usd === null ? "" : `$${(usage.cost_micro_usd / 1_000_000).toFixed(4)}`;
  const cost = !dollars
    ? ""
    : billing?.kind === "subscription"
      ? `, about ${dollars} at API prices (your plan's limits, not this amount)`
      : `, ${dollars}`;
  return `${usage.input_tokens.toLocaleString("en")} tokens in, ${usage.output_tokens.toLocaleString("en")} out${cost}`;
}

/** Whether a review item waits for the person, as the inbox counts it. */
function waits(item: ReviewItem): boolean {
  return (
    item.state === "partly_approved" ||
    (item.state === "open" && (item.git_changes.length > 0 || item.files.some((f) => f.by !== "outside")))
  );
}

/**
 * Opens the review inbox on the waiting item of the first of `sessions`
 * that has one; without an inbox that can, through its command.
 */
export function openReviews(sessions: SessionId[]): void {
  if (openInboxFor(sessions)) return;
  void listCommands()
    .find((c) => c.id === "inbox.toggle")
    ?.run();
}

const instances = new WeakMap<Transport, AgentSessions>();

export function agentSessions(transport: Transport): AgentSessions {
  let sessions = instances.get(transport);
  if (!sessions) {
    sessions = new AgentSessions(transport);
    instances.set(transport, sessions);
  }
  return sessions;
}

/** A session's name for people: its title, or its id while the title is unknown. */
function sessionName(info: SessionInfo | undefined, id: SessionId): string {
  return info?.title || `Session ${id}`;
}
