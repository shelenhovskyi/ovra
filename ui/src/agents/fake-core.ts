// Test-only fake of the core's agents and permissions: records calls, answers
// them like the core and lets a test push events.

import type {
  AgentMessage,
  AgentSetup,
  AgentStatus,
  CoreEvent,
  PermissionRequest,
  ProjectAgentConfig,
  ProjectId,
  Request,
  ResponseFor,
  SessionId,
  SessionInfo,
} from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export class FakeAgentCore implements Transport {
  calls: Request[] = [];
  sessions: SessionInfo[] = [];
  transcripts = new Map<SessionId, AgentMessage[]>();
  pending: PermissionRequest[] = [];
  /** What `agents.setup` answers; `agents.set_path` records the path in it. */
  setup: AgentSetup = { path: null, executable: "/usr/bin/claude", version: "2.1.331", state: { state: "ready" } };
  /** What `agents.project_config` reports as current. */
  config: ProjectAgentConfig = { fingerprint: "f0", items: [] };
  /** What the user trusted, per project. */
  trustedConfig = new Map<ProjectId, ProjectAgentConfig>();
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
        this.status(info.id, { state: "starting" });
        this.message(info.id, { kind: "user_text", text: prompt });
        return { type: "agents.session", data: { ...info } };
      }
      case "agents.transcript":
        return { type: "agents.transcript", data: this.transcripts.get(request.params.session) ?? [] };
      case "agents.send":
      case "agents.stop":
      case "agents.interrupt":
        return { type: "ok" };
      case "agents.setup":
        return { type: "agents.setup", data: { ...this.setup } };
      case "agents.set_path":
        this.setup = { ...this.setup, path: request.params.path };
        return { type: "agents.setup", data: { ...this.setup } };
      case "agents.project_config":
        return {
          type: "agents.project_config",
          data: { current: this.config, trusted: this.trustedConfig.get(request.params.project) ?? null },
        };
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
    };
    this.sessions.push(info);
    return info;
  }
}
