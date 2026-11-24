// Agent sessions the UI knows of, kept current from `agents.*` events. One
// instance per transport, shared by the agents panel, approvals and
// notifications (`agentSessions(transport)`).

import { SvelteMap } from "svelte/reactivity";
import type { AgentMessage, CoreEvent, ProjectId, SessionId, SessionInfo } from "../lib/protocol";
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

  async stop(session: SessionId): Promise<void> {
    await this.transport.call({ method: "agents.stop", params: { session } });
  }

  private apply(event: CoreEvent): void {
    if (event.type === "agents.status") {
      const { session, project, status } = event.data;
      const known = this.byId.get(session);
      if (known) {
        this.byId.set(session, { ...known, status });
        return;
      }
      if (status.state === "starting") this.transcripts.set(session, []);
      // Its title comes with the list (or the reply to agents.start).
      this.byId.set(session, { id: session, project, agent: "claude", mode: "direct", status, title: "" });
      void this.load(project).catch(() => {});
    } else if (event.type === "agents.message") {
      const { session, message } = event.data;
      const transcript = this.transcripts.get(session);
      if (transcript) this.transcripts.set(session, [...transcript, message]);
    }
  }

  private resync(): void {
    this.transcripts.clear();
    for (const project of this.loaded) void this.load(project).catch(() => {});
  }
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
