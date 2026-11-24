// Agent sessions the UI knows of, kept current from `agents.*` events. One
// instance per transport, shared by the agents panel, approvals and
// notifications (`agentSessions(transport)`).

import { SvelteMap } from "svelte/reactivity";
import type { AgentMessage, CoreEvent, ProjectId, SessionId, SessionInfo, TurnUsage } from "../lib/protocol";
import type { Transport } from "../lib/transport";

export class AgentSessions {
  /** Every known session by id, in the order they became known. */
  readonly byId = new SvelteMap<SessionId, SessionInfo>();
  /** Transcripts of sessions watched since they started or since `loadTranscript`. */
  readonly transcripts = new SvelteMap<SessionId, AgentMessage[]>();
  private readonly loaded = new Set<ProjectId>();

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
  }

  async loadTranscript(session: SessionId): Promise<void> {
    const { data } = await this.transport.call({ method: "agents.transcript", params: { session } });
    this.transcripts.set(session, data);
  }

  async start(project: ProjectId, prompt: string): Promise<SessionInfo> {
    // Messages may arrive before the reply: collect them from the start.
    const { data } = await this.transport.call({
      method: "agents.start",
      params: { project, agent: "claude", mode: "direct", prompt },
    });
    this.byId.set(data.id, { ...data, status: this.byId.get(data.id)?.status ?? data.status });
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
    else if (event.type === "agents.status") {
      const { session, project, status } = event.data;
      const known = this.byId.get(session);
      if (known) {
        this.byId.set(session, { ...known, status });
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
      });
      void this.load(project).catch(() => {});
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

/** `12,345 tokens in, 678 out, $0.0123` */
export function usageText(usage: TurnUsage): string {
  const cost = usage.cost_micro_usd === null ? "" : `, $${(usage.cost_micro_usd / 1_000_000).toFixed(4)}`;
  return `${usage.input_tokens.toLocaleString("en")} tokens in, ${usage.output_tokens.toLocaleString("en")} out${cost}`;
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
export function sessionName(info: SessionInfo | undefined, id: SessionId): string {
  return info?.title || `Session ${id}`;
}
