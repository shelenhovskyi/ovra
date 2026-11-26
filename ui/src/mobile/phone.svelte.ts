// What the phone shows, kept from the computer's answers and events. The
// computer decides what a phone may see and do (`crates/core/src/companion.rs`);
// this only reads it back.

import { SvelteMap } from "svelte/reactivity";
import { ApprovalQueue } from "../approvals/queue.svelte";
import type {
  AgentCapacity,
  AgentMessage,
  CoreEvent,
  KeepAwakeMode,
  PowerState,
  Project,
  ProjectId,
  Request,
  ReviewItem,
  SessionId,
  SessionInfo,
  Task,
} from "../lib/protocol";
import { type ConnectionStatus, CoreError, type Transport } from "../lib/transport";

/** A Web Push subscription as `PushSubscription.toJSON()` gives it. */
export type PushSubscriptionJson = { endpoint: string; keys: { p256dh: string; auth: string } };

/**
 * Requests only a phone sends; the phone listener answers them itself and
 * never passes them to the core (`crates/remote`).
 */
type PhoneRequest =
  | { method: "phone.forget" }
  | { method: "phone.push_key" }
  | { method: "phone.push_subscribe"; params: { subscription: PushSubscriptionJson | null } };

type PhoneResult<R extends PhoneRequest> = R extends { method: "phone.push_key" } ? { key: string } : null;

export function errorText(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

export class Phone {
  status = $state<ConnectionStatus>("connecting");
  projects = $state<Project[]>([]);
  sessions = $state<SessionInfo[]>([]);
  tasks = $state<Task[]>([]);
  reviews = $state<ReviewItem[]>([]);
  power = $state<PowerState | null>(null);
  readonly capacity = new SvelteMap<ProjectId, AgentCapacity>();
  /** Transcripts of the sessions opened on this phone. */
  readonly transcripts = new SvelteMap<SessionId, AgentMessage[]>();
  readonly approvals: ApprovalQueue;
  error = $state<string | null>(null);
  loaded = $state(false);

  constructor(readonly transport: Transport) {
    this.approvals = new ApprovalQueue(transport);
    transport.onEvent((event) => this.apply(event));
    if (transport.onStatus) {
      transport.onStatus((status) => {
        this.status = status;
        if (status === "open") void this.load();
      });
    } else {
      this.status = "open";
      void this.load();
    }
  }

  /** Sends a request only the phone listener knows. */
  phoneCall<R extends PhoneRequest>(request: R): Promise<PhoneResult<R>> {
    // Same frames as core requests; the listener answers these itself.
    return this.transport.call(request as unknown as Request) as unknown as Promise<PhoneResult<R>>;
  }

  async load(): Promise<void> {
    try {
      const [{ data: projects }, { data: power }] = await Promise.all([
        this.transport.call({ method: "projects.list" }),
        this.transport.call({ method: "power.get" }),
      ]);
      this.projects = projects;
      this.power = power;
      const lists = await Promise.all(projects.map((p) => this.loadProject(p.id)));
      this.sessions = lists.flatMap((l) => l.sessions);
      this.tasks = lists.flatMap((l) => l.tasks);
      this.reviews = lists.flatMap((l) => l.reviews);
      for (const id of this.transcripts.keys()) void this.loadTranscript(id);
      this.error = null;
      this.loaded = true;
    } catch (err) {
      this.error = `Could not load from the computer: ${errorText(err)}`;
    }
  }

  private async loadProject(project: ProjectId) {
    const [sessions, tasks, reviews, capacity] = await Promise.all([
      this.transport.call({ method: "agents.list", params: { project } }),
      this.transport.call({ method: "orchestration.list", params: { project } }),
      this.transport.call({ method: "review.list", params: { project } }),
      this.transport.call({ method: "agents.capacity", params: { project } }),
    ]);
    this.capacity.set(project, capacity.data);
    return { sessions: sessions.data, tasks: tasks.data, reviews: reviews.data };
  }

  private async reloadProject(project: ProjectId): Promise<void> {
    try {
      const { sessions, tasks, reviews } = await this.loadProject(project);
      this.sessions = [...this.sessions.filter((s) => s.project !== project), ...sessions];
      this.tasks = [...this.tasks.filter((t) => t.project !== project), ...tasks];
      this.reviews = [...this.reviews.filter((r) => r.project !== project), ...reviews];
    } catch (err) {
      this.error = `Could not load from the computer: ${errorText(err)}`;
    }
  }

  async loadTranscript(session: SessionId): Promise<void> {
    try {
      const { data } = await this.transport.call({ method: "agents.transcript", params: { session } });
      this.transcripts.set(session, data);
    } catch (err) {
      this.error = `Could not load the conversation: ${errorText(err)}`;
    }
  }

  project(id: ProjectId): Project | undefined {
    return this.projects.find((p) => p.id === id);
  }

  session(id: SessionId): SessionInfo | undefined {
    return this.sessions.find((s) => s.id === id);
  }

  /** Sends `text` to the agent as the person's reply; `false` if it failed. */
  async reply(session: SessionId, text: string): Promise<boolean> {
    return this.run(() => this.transport.call({ method: "agents.send", params: { session, text } }));
  }

  /** Stops the turn that runs now; the session stays. */
  interrupt(session: SessionId): Promise<boolean> {
    return this.run(() => this.transport.call({ method: "agents.interrupt", params: { session } }));
  }

  /** Ends the session. */
  stop(session: SessionId): Promise<boolean> {
    return this.run(() => this.transport.call({ method: "agents.stop", params: { session } }));
  }

  setMode(mode: KeepAwakeMode): Promise<boolean> {
    return this.run(async () => {
      this.power = (await this.transport.call({ method: "power.set_mode", params: { mode } })).data;
    });
  }

  private async run(work: () => Promise<unknown>): Promise<boolean> {
    try {
      await work();
      this.error = null;
      return true;
    } catch (err) {
      this.error = errorText(err);
      return false;
    }
  }

  private apply(event: CoreEvent): void {
    switch (event.type) {
      case "info.resync":
        void this.load();
        return;
      case "agents.status": {
        const { session, project, status } = event.data;
        if (this.sessions.some((s) => s.id === session)) {
          this.sessions = this.sessions.map((s) => (s.id === session ? { ...s, status } : s));
        } else void this.reloadProject(project);
        return;
      }
      case "agents.message": {
        const kept = this.transcripts.get(event.data.session);
        if (kept) this.transcripts.set(event.data.session, [...kept, event.data.message]);
        return;
      }
      case "orchestration.changed":
      case "review.changed":
        void this.reloadProject(event.data.project);
        return;
      case "agents.limit_changed":
        for (const project of this.capacity.keys()) void this.reloadProject(project);
        return;
      case "power.changed":
        this.power = event.data;
        return;
      default:
        return;
    }
  }
}

/** Sessions that wait for the person to answer an approval. */
export function needsYou(phone: Phone): SessionInfo[] {
  const asking = new Set(phone.approvals.pending.map((r) => r.session));
  return phone.sessions.filter((s) => asking.has(s.id) || s.status.state === "waiting_for_approval");
}

/** Sessions in a turn or waiting to start. */
export function working(phone: Phone): SessionInfo[] {
  const busy = new Set(["queued", "preparing", "starting", "working"]);
  const asking = new Set(phone.approvals.pending.map((r) => r.session));
  return phone.sessions.filter((s) => busy.has(s.status.state) && !asking.has(s.id));
}

/** Sessions whose turn ended: done (they take a reply), stopped or failed. */
export function finished(phone: Phone): SessionInfo[] {
  const ended = new Set(["idle", "stopped", "failed"]);
  const asking = new Set(phone.approvals.pending.map((r) => r.session));
  return phone.sessions.filter((s) => ended.has(s.status.state) && !asking.has(s.id));
}

/** Work an agent asked the person to review (`request_review`) that is not decided yet. */
export function forReview(phone: Phone): ReviewItem[] {
  return phone.reviews.filter((r) => r.state === "open" && !!r.summary);
}
