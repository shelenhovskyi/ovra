// Test-only fake of the core's agents and permissions: records calls, answers
// them like the core and lets a test push events.

import type {
  AgentMessage,
  AgentStatus,
  CoreEvent,
  PermissionRequest,
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
    };
    this.sessions.push(info);
    return info;
  }
}
