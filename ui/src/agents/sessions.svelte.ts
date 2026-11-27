// Agent sessions the UI knows of, kept current from `agents.*` events. One
// instance per transport, shared by the agents panel, approvals and
// notifications (`agentSessions(transport)`).

import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { openInboxFor } from "../inbox/open";
import { listCommands } from "../keys/commands";
import type {
  AgentCapacity,
  AgentKind,
  AgentMessage,
  AgentModel,
  Billing,
  ContextItem,
  CoreEvent,
  DiskUsage,
  FailedWorker,
  FileChange,
  Leftover,
  NotResumed,
  Pressure,
  ProjectId,
  ReviewItem,
  SessionId,
  SessionInfo,
  SessionResources,
  SetupProgress,
  Task,
  TaskId,
  TurnUsage,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { failedLabel } from "./errors";
import { hints } from "./hints.svelte";
import { providerName } from "./providers.svelte";
import { pressureNote, waitingLabel } from "./resources";
import { isBusy, isEnded, statusLabel } from "./status";
import { diskTotal, preparingLabel, waitingReason } from "./worktrees";

/** A session's part in a task, if it has one. */
export type SessionRole =
  | { kind: "coordinator"; task: TaskId }
  | {
      kind: "worker";
      task: TaskId;
      name: string;
      branch: string;
      /** Absolute path of its copy of the project, where it works. */
      worktree: string;
      /** Why its worktree stays although its task ended. */
      kept: string | null;
      /** Its worktree's size on disk, once counted. */
      disk: DiskUsage | null;
      /** Its worktree's setup while it runs and after it failed. */
      setup: SetupProgress | null;
      /** Why it waits queued although a place may be free (low disk space). */
      waiting: string | null;
      /** Paths copied into its worktree through `.worktreeinclude`. */
      included: string[];
      /** `null` for one agent in its own copy, which has no coordinator. */
      coordinator: SessionId | null;
      /** One of several attempts at one prompt: the first attempt's task and its place, from 1. */
      attempt: { first: TaskId; number: number } | null;
    };

/** The `model` param for a start: only Claude takes one, and none means the account's default. */
function modelFor(agent: AgentKind, model: AgentModel | null): { model?: AgentModel } {
  return agent === "claude" && model ? { model } : {};
}

export class AgentSessions {
  /** Coordinators and workers of the projects' tasks. */
  readonly roles = new SvelteMap<SessionId, SessionRole>();
  /** Workers each task's coordinator asked for that could not start, by coordinator. */
  readonly failedStarts = new SvelteMap<SessionId, FailedWorker[]>();
  /** The disk the workers' worktrees of each project use, once any was counted. */
  readonly disk = new SvelteMap<ProjectId, DiskUsage>();
  /** Folders in each project's worktrees folder that no task knows, until the person removes them. */
  readonly leftovers = new SvelteMap<ProjectId, Leftover[]>();
  /** Every known session by id, in the order they became known. */
  readonly byId = new SvelteMap<SessionId, SessionInfo>();
  /** Transcripts of sessions watched since they started or since `loadTranscript`. */
  readonly transcripts = new SvelteMap<SessionId, AgentMessage[]>();
  /** The number of the last message in each of `transcripts`: a message event at or below it is already there. */
  private readonly seqs = new Map<SessionId, number>();
  /** Each session's newest review item: what its agent changed, kept or not. */
  readonly reviews = new SvelteMap<SessionId, ReviewItem>();
  /** Sessions with an item that waits for the person, counted as the inbox counts them. */
  private readonly waitingReview = new SvelteSet<SessionId>();
  /** How many of each loaded project's agents work, of how many may. */
  readonly capacity = new SvelteMap<ProjectId, AgentCapacity>();
  /** What each running session takes of the computer, from the latest `resources.sample`. */
  readonly resources = new SvelteMap<SessionId, SessionResources>();
  /** Why each project's queued workers wait although a place may be free; absent when they need not. */
  readonly pressure = new SvelteMap<ProjectId, Pressure>();
  /** The text each session's agent is writing now (`agents.text_delta`), until its message comes. */
  readonly partial = new SvelteMap<SessionId, string>();
  /**
   * Sessions the person closed: hidden from the list, remembered in this
   * browser per project. The core keeps them (it has no way to forget one).
   */
  readonly closed = new SvelteSet<SessionId>();
  /** Tool calls the person denied, by session, tool and input (`callKey`), while this window watched. */
  private readonly deniedCalls = new SvelteSet<string>();
  /** Requests seen asking, until their answer comes: what each one asked to run. */
  private readonly asking = new Map<string, string>();
  private readonly loaded = new Set<ProjectId>();
  /** The latest `agents.capacity` call per project: older replies are stale. */
  private readonly asked = new Map<ProjectId, number>();
  /** The `load` per project that runs now. */
  private readonly loads = new Map<ProjectId, Promise<void>>();
  /** The `review.list` reading per project that runs now, and whether another must follow it. */
  private readonly reviewsLoading = new Map<ProjectId, { again: boolean; done: Promise<void> }>();

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

  load(project: ProjectId): Promise<void> {
    const load = this.read(project).finally(() => {
      if (this.loads.get(project) === load) this.loads.delete(project);
    });
    this.loads.set(project, load);
    return load;
  }

  /** Like `load`, but shares a load of `project` that runs now: panels that open together ask once. */
  show(project: ProjectId): Promise<void> {
    return this.loads.get(project) ?? this.load(project);
  }

  private async read(project: ProjectId): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.list", params: { project } });
    this.loaded.add(project);
    for (const id of readClosed(project)) this.closed.add(id);
    for (const info of data) this.byId.set(info.id, info);
    await Promise.all([
      this.loadTasks(project),
      this.loadReviews(project),
      this.loadCapacity(project),
      this.loadLeftovers(project),
    ]);
  }

  /**
   * Each session's newest review item. Front ends without reviews have none.
   * Calls while one runs share one more reading after it: workers that write
   * send `review.changed` every second, and each `review.list` looks at every
   * open item's worktree.
   */
  loadReviews(project: ProjectId): Promise<void> {
    const running = this.reviewsLoading.get(project);
    if (running) {
      running.again = true;
      return running.done;
    }
    const load = { again: false, done: Promise.resolve() };
    this.reviewsLoading.set(project, load);
    load.done = (async () => {
      try {
        do {
          load.again = false;
          await this.readReviews(project);
        } while (load.again);
      } finally {
        this.reviewsLoading.delete(project);
      }
    })();
    return load.done;
  }

  private async readReviews(project: ProjectId): Promise<void> {
    let items: ReviewItem[];
    try {
      ({ data: items } = await this.transport.call({ method: "review.list", params: { project } }));
    } catch {
      return;
    }
    for (const [session, item] of this.reviews)
      if (item.project === project) {
        this.reviews.delete(session);
        this.waitingReview.delete(session);
      }
    // Newest first: the first item of a session is its current one.
    for (const item of items) {
      if (!this.reviews.has(item.session)) this.reviews.set(item.session, item);
      if (waits(item)) this.waitingReview.add(item.session);
    }
  }

  /** Whether any of `id`'s review items waits for the person, as the inbox counts it. */
  waitsForReview(id: SessionId): boolean {
    return this.waitingReview.has(id);
  }

  /**
   * Whether the person denied `session`'s call of `tool` with `input`: the
   * agent may get their message rather than the CLI's words for a denial.
   */
  denied(session: SessionId, tool: string, input: unknown): boolean {
    return this.deniedCalls.has(callKey(session, tool, input));
  }

  /** The files `id`'s agent changed that wait for the person's review. */
  changes(id: SessionId): FileChange[] {
    const item = this.reviews.get(id);
    return item && waits(item) ? item.files.filter((f) => f.by !== "outside") : [];
  }

  /** How many of the task's workers have changes waiting for review. */
  waitingInTask(coordinator: SessionId): number {
    return this.workers(coordinator).filter((session) => this.waitsForReview(session)).length;
  }

  /** The task's workers, in the order it started them. */
  workers(coordinator: SessionId): SessionId[] {
    return [...this.roles]
      .filter(([, role]) => role.kind === "worker" && role.coordinator === coordinator)
      .map(([session]) => session);
  }

  /**
   * Coordinators of `project` that plan or direct now: they work but take
   * no place in `capacity` (the cap counts the agents that change files).
   */
  planning(project: ProjectId): number {
    return this.inProject(project).filter((s) => this.roles.get(s.id)?.kind === "coordinator" && isBusy(s.status))
      .length;
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
      const total = diskTotal(data.flatMap((task) => task.workers.map((w) => w.disk)));
      if (total) this.disk.set(project, total);
      else this.disk.delete(project);
      // Several solo tasks trying one prompt, numbered in the order they started.
      const tries = new Map<TaskId, TaskId[]>();
      for (const task of data) {
        const first = task.attempt_of ?? task.id;
        tries.set(first, [...(tries.get(first) ?? []), task.id]);
      }
      const attemptOf = (task: TaskId) => {
        for (const [first, all] of tries)
          if (all.length > 1 && all.includes(task)) return { first, number: all.indexOf(task) + 1 };
        return null;
      };
      for (const task of data) {
        if (task.coordinator !== null) {
          this.roles.set(task.coordinator, { kind: "coordinator", task: task.id });
          this.failedStarts.set(task.coordinator, task.failed ?? []);
        }
        for (const w of task.workers)
          this.roles.set(w.session, {
            kind: "worker",
            task: task.id,
            name: w.name,
            branch: w.branch,
            worktree: w.worktree,
            kept: w.kept,
            disk: w.disk ?? null,
            setup: w.setup ?? null,
            waiting: w.waiting ?? null,
            included: w.included ?? [],
            coordinator: task.coordinator,
            attempt: attemptOf(task.id),
          });
      }
    } catch {
      // No tasks to show.
    }
  }

  /** Front ends without leftovers list none. */
  private async loadLeftovers(project: ProjectId): Promise<void> {
    try {
      const { data } = await this.transport.call({ method: "orchestration.leftovers", params: { project } });
      this.leftovers.set(project, data);
    } catch {
      // None to show.
    }
  }

  /** Removes a leftover worktree: only ever when the person asked, after a confirmation. */
  async removeLeftover(project: ProjectId, path: string): Promise<void> {
    await this.transport.call({ method: "orchestration.remove_leftover", params: { project, path } });
    await this.loadLeftovers(project);
  }

  /** Runs a worker's failed setup again, or with `skip` starts it without (Start anyway). */
  async retrySetup(session: SessionId, skip: boolean): Promise<void> {
    await this.transport.call({ method: "orchestration.retry_setup", params: { session, skip_setup: skip } });
  }

  /**
   * A session's name for people: coordinators as the task, workers by
   * their name and their task's (`docs · Fix login`), so the workers of two
   * tasks differ.
   */
  name(id: SessionId): string {
    const role = this.roles.get(id);
    const base = sessionName(this.byId.get(id), id);
    // One agent in its own copy is named by its own title.
    if (role?.kind === "worker" && role.coordinator !== null) {
      const task = this.byId.get(role.coordinator)?.title;
      return task ? `${role.name} · ${task}` : `Worker ${role.name}`;
    }
    if (role?.kind === "coordinator") return `Task: ${base}`;
    if (role?.kind === "worker" && role.attempt) {
      const agent = this.byId.get(id)?.agent;
      return `Attempt ${role.attempt.number}${agent ? ` (${providerName(agent)})` : ""} · ${base}`;
    }
    return base;
  }

  /** The other attempts at the prompt `id` tries, if it is one of several. */
  attempts(id: SessionId): SessionId[] {
    const role = this.roles.get(id);
    const first = role?.kind === "worker" ? role.attempt?.first : undefined;
    if (!first) return [];
    return [...this.roles]
      .filter(([other, r]) => other !== id && r.kind === "worker" && r.attempt?.first === first)
      .map(([other]) => other);
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
      if (this.workers(id).some((w) => this.setupFailed(w))) return "A worker's setup failed";
      if (workers.some(isBusy)) return "Workers are on it";
      if (workers.some((s) => s?.state === "queued")) {
        const pressure = this.pressure.get(info.project);
        return pressure ? `Workers are waiting: ${pressureNote(pressure)}` : "Workers are waiting for a free agent";
      }
      // Never done while a worker failed or did not start.
      const missed = this.failedStarts.get(id)?.length ?? 0;
      const failed = missed + workers.filter((s) => s?.state === "failed").length;
      if (failed > 0) return `Failed: ${failed} of ${workers.length + missed} workers`;
    }
    if (info.interrupted && isEnded(info.status)) return "Stopped when Ovra closed";
    const transcript = this.transcripts.get(id);
    // Stopping a worker that had finished (its task was stopped) does not undo its work.
    if (role?.kind === "worker" && info.status.state === "stopped" && transcript?.at(-1)?.kind === "turn_end")
      return failedLabel(info.status, transcript) ?? "Done";
    // Nor a task whose workers all finished: Done stays Done.
    if (role?.kind === "coordinator" && info.status.state === "stopped" && transcript?.at(-1)?.kind === "turn_end") {
      const workers = this.workers(id);
      const missed = this.failedStarts.get(id)?.length ?? 0;
      if (workers.length > 0 && missed === 0 && workers.every((w) => this.status(w) === "Done"))
        return failedLabel(info.status, transcript) ?? "Done";
    }
    if (info.status.state === "preparing" && role?.kind === "worker") return preparingLabel(role.setup);
    // A worker waits for disk space for its worktree.
    if (info.status.state === "queued" && role?.kind === "worker" && role.waiting) return waitingReason(role.waiting);
    // A free place does not start it while the computer is short of memory or CPU.
    const pressure = this.pressure.get(info.project);
    if (info.status.state === "queued" && pressure) return waitingLabel(pressure);
    if (info.status.state === "queued") {
      const place = this.capacity.get(info.project)?.waiting.indexOf(id) ?? -1;
      if (place >= 0) return `Waiting, ${ordinal(place + 1)} in line`;
    }
    return failedLabel(info.status, transcript) ?? statusLabel(info.status);
  }

  /** Whether `id` is a worker whose setup failed: it waits for Retry or Start anyway. */
  setupFailed(id: SessionId): boolean {
    const role = this.roles.get(id);
    return role?.kind === "worker" && this.byId.get(id)?.status.state === "preparing" && !!role.setup?.failed;
  }

  /** `grouped` without the sessions the person closed. */
  shown(project: ProjectId): SessionInfo[] {
    return this.grouped(project).filter((s) => !this.closed.has(s.id));
  }

  /**
   * Hides a session that is done; one that still waits for replies is
   * stopped first. A task's coordinator takes its workers along.
   */
  async close(id: SessionId): Promise<void> {
    const info = this.byId.get(id);
    if (!info || isBusy(info.status)) return;
    if (!isEnded(info.status)) await this.stop(id);
    this.closed.add(id);
    // A task closes with its workers; stopping its coordinator stopped them.
    if (this.roles.get(id)?.kind === "coordinator") for (const worker of this.workers(id)) this.closed.add(worker);
    this.saveClosed(info.project);
  }

  /** Shows a closed session in its project's list again (History opened it). */
  reopen(id: SessionId): void {
    const info = this.byId.get(id);
    if (!info || !this.closed.delete(id)) return;
    this.saveClosed(info.project);
  }

  /**
   * Hides a session another panel started for a purpose of its own (the git
   * panel's agent actions), at once and in whatever state it is: that panel
   * shows the result and stops the session itself.
   */
  hide(project: ProjectId, id: SessionId): void {
    this.closed.add(id);
    try {
      const kept = new Set(readClosed(project));
      kept.add(id);
      localStorage.setItem(closedKey(project), JSON.stringify([...kept]));
    } catch {
      // Storage may be full or blocked: hidden for this window only.
    }
  }

  private saveClosed(project: ProjectId): void {
    const ids = this.inProject(project)
      .map((s) => s.id)
      .filter((id) => this.closed.has(id));
    try {
      localStorage.setItem(closedKey(project), JSON.stringify(ids));
    } catch {
      // Storage may be full or blocked: closed for this window only.
    }
  }

  /** Sessions of `project` with each task's workers right after their coordinator, in the order it started them. */
  grouped(project: ProjectId): SessionInfo[] {
    const all = this.inProject(project);
    const workersOf = (coordinator: SessionId) =>
      this.workers(coordinator).flatMap((id) => all.filter((s) => s.id === id));
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
  async startTask(
    project: ProjectId,
    prompt: string,
    context: ContextItem[] = [],
    agent: AgentKind = "claude",
    model: AgentModel | null = null,
  ): Promise<SessionId> {
    const { data } = await this.transport.call({
      method: "orchestration.start",
      // Left out, the core runs Claude.
      params: {
        project,
        prompt,
        ...(agent === "claude" ? {} : { agent }),
        ...(context.length > 0 ? { context } : {}),
        ...modelFor(agent, model),
      },
    });
    await this.load(project);
    return data.coordinator ?? data.workers[0]?.session ?? "";
  }

  /**
   * Starts one agent of `agent` in its own worktree, without a coordinator
   * ("One agent, own copy"). Resolves with its session.
   */
  async startSolo(
    project: ProjectId,
    prompt: string,
    agent: AgentKind = "claude",
    context: ContextItem[] = [],
    model: AgentModel | null = null,
  ): Promise<SessionId> {
    return this.startAttempts(project, prompt, [agent], context, model);
  }

  /**
   * Starts one attempt at `prompt` per agent in `agents`, each in its own
   * worktree (`docs/specs/attempts.md`); one agent is "One agent, own copy".
   * Resolves with the first attempt's session. When a later one cannot start,
   * the ones before keep working and the error says which attempt failed.
   */
  async startAttempts(
    project: ProjectId,
    prompt: string,
    agents: AgentKind[],
    context: ContextItem[] = [],
    model: AgentModel | null = null,
    planFirst = false,
    repos: ProjectId[] = [],
  ): Promise<SessionId> {
    const once = async (agent: AgentKind, attemptOf?: TaskId): Promise<Task> => {
      const { data } = await this.transport.call({
        method: "orchestration.start",
        params: {
          project,
          prompt,
          agent,
          solo: true,
          ...(context.length > 0 ? { context } : {}),
          ...modelFor(agent, model),
          ...(attemptOf ? { attempt_of: attemptOf } : {}),
          ...(planFirst ? { plan_first: true } : {}),
          // Other repositories go with one attempt only (`docs/specs/multi-repo.md`).
          ...(repos.length > 0 && !attemptOf ? { repos } : {}),
        },
      });
      return data;
    };
    const [lead = "claude", ...more] = agents;
    let first: Task;
    try {
      first = await once(lead);
      for (const [at, agent] of more.entries()) {
        try {
          await once(agent, first.id);
        } catch (err) {
          const why = err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
          throw new Error(
            `attempt ${at + 2} of ${agents.length} did not start (${why}); the ones before it are working`,
          );
        }
      }
    } finally {
      await this.load(project);
    }
    const session = first.workers[0]?.session;
    if (!session) throw new Error("The agent did not start");
    return session;
  }

  async loadTranscript(session: SessionId): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.transcript", params: { session } });
    this.seqs.set(session, data.seq);
    this.transcripts.set(session, data.messages);
  }

  /** `account`: a Claude account's id; null uses the project's. `research` only reads and searches (ADR 0013). */
  async start(
    project: ProjectId,
    prompt: string,
    account: string | null = null,
    mode: "direct" | "research" = "direct",
    context: ContextItem[] = [],
    agent: AgentKind = "claude",
    model: AgentModel | null = null,
    planFirst = false,
  ): Promise<SessionInfo> {
    // Messages may arrive before the reply: collect them from the start.
    const { data } = await this.transport.call({
      method: "agents.start",
      params: {
        project,
        agent,
        mode,
        prompt,
        account,
        ...(context.length > 0 ? { context } : {}),
        ...modelFor(agent, model),
        ...(planFirst ? { plan_first: true } : {}),
      },
    });
    const known = this.byId.get(data.id);
    // agents.sandbox may also have come before the reply: keep what it said.
    const sandbox = known?.sandbox_reported ? { sandbox: known.sandbox, sandbox_reported: true } : {};
    this.byId.set(data.id, { ...data, status: known?.status ?? data.status, ...sandbox });
    return data;
  }

  async send(session: SessionId, text: string, context: ContextItem[] = []): Promise<void> {
    const params = { session, text, ...(context.length > 0 ? { context } : {}) };
    await this.transport.call({ method: "agents.send", params });
    this.patch(session, { can_undo_go_back: false });
  }

  /** Sends `text` once the running turn ends (or now, if none runs) instead of interrupting the agent. */
  async queue(session: SessionId, text: string, context: ContextItem[] = []): Promise<void> {
    const params = { session, text, ...(context.length > 0 ? { context } : {}) };
    await this.transport.call({ method: "agents.queue", params });
  }

  /** Takes a queued message back; fails once it was sent. */
  async unqueue(session: SessionId, id: number): Promise<void> {
    await this.transport.call({ method: "agents.unqueue", params: { session, id } });
  }

  /** Approves a planning session's plan: it may change files and goes ahead. */
  async approvePlan(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.approve_plan", params: { session } });
  }

  /** The turns of a direct session that can be gone back to; none when the core no longer tracks it. */
  async checkpoints(session: SessionId): Promise<Set<number>> {
    try {
      const { data } = await this.transport.call({ method: "agents.checkpoints", params: { session } });
      return new Set(data.map((c) => c.turn));
    } catch {
      return new Set();
    }
  }

  /** Sets the agent's changes since before `turn` back (`docs/specs/checkpoints.md`). */
  async goBack(session: SessionId, turn: number): Promise<{ restored: string[]; conflicts: string[] }> {
    const { data } = await this.transport.call({ method: "agents.go_back", params: { session, turn } });
    if (data.can_undo) this.patch(session, { undone_from: turn, can_undo_go_back: true });
    await this.reviewAgain(session);
    return data;
  }

  /** Puts back what the last go-back restored. */
  async undoGoBack(session: SessionId): Promise<{ restored: string[]; conflicts: string[] }> {
    const { data } = await this.transport.call({ method: "agents.undo_go_back", params: { session } });
    this.patch(session, { undone_from: null, can_undo_go_back: false });
    await this.reviewAgain(session);
    return data;
  }

  /** Files were set back: the review item's changed files are read again. */
  private async reviewAgain(session: SessionId): Promise<void> {
    const project = this.byId.get(session)?.project;
    if (project) await this.loadReviews(project);
  }

  /** The core changed these fields without an event of their own. */
  private patch(session: SessionId, fields: Partial<SessionInfo>): void {
    const known = this.byId.get(session);
    if (known) this.byId.set(session, { ...known, ...fields });
  }

  /** Ends the current turn; the agent's pending requests are denied and it waits for a message. */
  async interrupt(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.interrupt", params: { session } });
  }

  /** Continues an ended session with a new prompt; it keeps its id and transcript. */
  async resume(session: SessionId, prompt: string): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.resume", params: { session, prompt } });
    this.byId.set(data.id, { ...data, status: this.byId.get(data.id)?.status ?? data.status, can_undo_go_back: false });
  }

  /**
   * Continues the sessions Ovra's quit cut off: `session`, or every one of
   * `project`'s. Answers why those that could not go on did not.
   */
  async resumeInterrupted(project: ProjectId, session?: SessionId): Promise<NotResumed[]> {
    const { data } = await this.transport.call({
      method: "agents.resume_interrupted",
      params: session ? { project, session } : { project },
    });
    for (const id of data.resumed) this.patch(id, { interrupted: false, resumable: false, can_undo_go_back: false });
    return data.left;
  }

  async stop(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.stop", params: { session } });
  }

  private apply(event: CoreEvent): void {
    // Events were dropped: what is on screen may be missing or doubled.
    if (event.type === "info.resync") this.resync();
    else if (event.type === "permissions.requested") {
      const { id, session, tool, input } = event.data;
      this.asking.set(id, callKey(session, tool, input));
    } else if (event.type === "permissions.resolved") {
      const call = this.asking.get(event.data.id);
      this.asking.delete(event.data.id);
      if (call && event.data.decision === "deny") this.deniedCalls.add(call);
    } else if (event.type === "orchestration.changed") {
      void this.loadTasks(event.data.project);
      // A task that starts takes places before its workers have a state.
      if (this.loaded.has(event.data.project)) void this.loadCapacity(event.data.project);
    } else if (event.type === "review.changed") {
      // A worker that asked for review stops counting, and counts again once it is decided.
      if (this.loaded.has(event.data.project))
        void Promise.all([this.loadReviews(event.data.project), this.loadCapacity(event.data.project)]);
    } else if (event.type === "agents.limit_changed" || event.type === "agents.research_limit_changed") {
      // "2 of 3 working" names the limit.
      for (const project of this.loaded) void this.loadCapacity(project);
    } else if (event.type === "resources.sample") {
      const { project, pressure, sessions } = event.data;
      if (pressure) this.pressure.set(project, pressure);
      else this.pressure.delete(project);
      const now = new Set(sessions.map((r) => r.session));
      for (const [id] of this.resources)
        if (this.byId.get(id)?.project === project && !now.has(id)) this.resources.delete(id);
      for (const r of sessions) this.resources.set(r.session, r);
    } else if (event.type === "agents.text_delta") {
      const { session, text } = event.data;
      this.partial.set(session, (this.partial.get(session) ?? "") + text);
    } else if (event.type === "agents.status") {
      const { session, project, status } = event.data;
      // Any change of state may change how many work.
      if (this.loaded.has(project)) void this.loadCapacity(project);
      if (status.state !== "working") this.partial.delete(session);
      if (isEnded(status)) this.resources.delete(session);
      const known = this.byId.get(session);
      // A turn ended: what it changed is in the session's review item now.
      if (known && status.state === "idle") void this.loadReviews(project);
      // A closed session that works again (resumed elsewhere) shows again.
      if (known && isBusy(status) && this.closed.delete(session)) this.saveClosed(project);
      if (known) {
        // A restart clears the sandbox report without an event of its own.
        const fresh = status.state === "starting" ? { sandbox: null, sandbox_reported: false } : {};
        this.byId.set(session, { ...known, status, ...fresh });
        // Whether it can be resumed comes only with the list.
        if (status.state === "stopped" || status.state === "failed") void this.load(project).catch(() => {});
        return;
      }
      if (status.state === "starting") {
        this.seqs.set(session, 0);
        this.transcripts.set(session, []);
      }
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
    } else if (event.type === "agents.stuck") {
      this.patch(event.data.session, { stuck: event.data.stuck });
    } else if (event.type === "agents.billing") {
      const known = this.byId.get(event.data.session);
      if (known) this.byId.set(event.data.session, { ...known, billing: event.data.billing });
    } else if (event.type === "agents.queue") {
      this.patch(event.data.session, { queue: event.data.queue });
    } else if (event.type === "agents.planning") {
      this.patch(event.data.session, { planning: event.data.planning });
    } else if (event.type === "agents.sandbox") {
      const known = this.byId.get(event.data.session);
      if (known) this.byId.set(event.data.session, { ...known, sandbox: event.data.sandbox, sandbox_reported: true });
    } else if (event.type === "agents.message") {
      const { session, message } = event.data;
      // The complete message replaces the text that streamed in.
      if (
        message.kind === "assistant_text"
          ? !message.parent
          : ["turn_end", "interrupted", "error"].includes(message.kind)
      )
        this.partial.delete(session);
      const transcript = this.transcripts.get(session);
      // The reply to agents.transcript can overtake this event and hold the message already.
      if (transcript && event.data.seq > (this.seqs.get(session) ?? 0)) {
        this.seqs.set(session, event.data.seq);
        this.transcripts.set(session, [...transcript, message]);
      }
      const known = this.byId.get(session);
      if (known && message.kind === "turn_end")
        this.byId.set(session, { ...known, usage: add(known.usage, message.usage) });
    }
  }

  private resync(): void {
    this.transcripts.clear();
    this.seqs.clear();
    this.partial.clear();
    for (const project of this.loaded) void this.load(project).catch(() => {});
  }
}

/** One tool call of one session: the permission request and the transcript's call carry the same input. */
function callKey(session: SessionId, tool: string, input: unknown): string {
  return `${session}\n${tool}\n${JSON.stringify(input)}`;
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
  hints.mark("reviewed");
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

function closedKey(project: ProjectId): string {
  return `ovra.agents.closed.${project}`;
}

function readClosed(project: ProjectId): SessionId[] {
  try {
    const ids: unknown = JSON.parse(localStorage.getItem(closedKey(project)) ?? "[]");
    return Array.isArray(ids) ? ids.filter((id): id is SessionId => typeof id === "string") : [];
  } catch {
    return [];
  }
}

/** A session's name for people: its title, or its id while the title is unknown. */
function sessionName(info: SessionInfo | undefined, id: SessionId): string {
  return info?.title || `Session ${id}`;
}

/** 1st, 2nd, 3rd, 4th, ... 11th, 12th, 13th, 21st. */
function ordinal(n: number): string {
  const teen = n % 100 >= 11 && n % 100 <= 13;
  const suffix = teen ? "th" : (({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th");
  return `${n}${suffix}`;
}
