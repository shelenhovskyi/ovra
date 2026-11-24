// Turns agent events into notifications: a permission request needs the
// person, a finished turn waits for them, a failure is an error. A task's
// sessions (its coordinator and workers) finish together: one notification
// when none of them works any more, never one per session while others run.

import { agentSessions } from "../agents/sessions.svelte";
import { describeTool } from "../approvals/describe";
import type { AgentStatus, PermissionId, ProjectId, SessionId, Task } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Notifier } from "./notifier.svelte";

/** Listens until the returned function is called. `review` shows one pending request. */
export function watchAgents(transport: Transport, notifier: Notifier, review: (id: PermissionId) => void): () => void {
  const sessions = agentSessions(transport);
  const last = new Map<SessionId, AgentStatus["state"]>();
  const name = (id: SessionId) => sessions.name(id);
  const state = (id: SessionId) => last.get(id) ?? sessions.byId.get(id)?.status.state;
  const busy = (id: SessionId) => ["starting", "working", "waiting_for_approval"].includes(state(id) ?? "");

  /** The task `session` belongs to, asked of the core: the roles known here may lag behind a spawn. */
  async function taskOf(project: ProjectId, session: SessionId): Promise<Task | undefined> {
    try {
      const { data } = await transport.call({ method: "orchestration.list", params: { project } });
      return data.find((t) => t.coordinator === session || t.workers.some((w) => w.session === session));
    } catch {
      return undefined; // A front end without tasks.
    }
  }

  async function finished(project: ProjectId, session: SessionId): Promise<void> {
    const task = await taskOf(project, session);
    if (!task) {
      if (state(session) === "idle") {
        notifier.notify({
          key: `session:${session}`,
          level: "info",
          text: `${name(session)} finished and waits for your reply`,
        });
      }
      return;
    }
    const members = [task.coordinator, ...task.workers.map((w) => w.session)];
    if (members.some(busy)) return;
    notifier.notify({
      key: `task:${task.id}`,
      level: "info",
      text: `${name(task.coordinator)} finished: none of its agents is working`,
    });
  }

  return transport.onEvent((event) => {
    switch (event.type) {
      case "permissions.requested": {
        const request = event.data;
        const action = describeTool(request.tool, request.input).action;
        notifier.notify(
          {
            key: `permission:${request.id}`,
            level: "attention",
            text: `${name(request.session)} needs approval: ${action}`,
            action: { label: "Review", run: () => review(request.id) },
          },
          "Approval needed",
        );
        break;
      }
      case "permissions.resolved":
        notifier.dismissKey(`permission:${event.data.id}`);
        break;
      case "agents.status": {
        const { session, project, status } = event.data;
        const before = last.get(session);
        last.set(session, status.state);
        const task = sessions.roles.get(session)?.task;
        // Work again: an earlier "finished" is no longer true.
        if (task && busy(session)) notifier.dismissKey(`task:${task}`);
        if (status.state === "idle" && (before === "working" || before === "waiting_for_approval")) {
          void finished(project, session);
        } else if (status.state === "failed") {
          notifier.notify(
            { key: `session:${session}`, level: "error", text: `${name(session)} failed: ${status.message}` },
            "Agent failed",
          );
        }
        break;
      }
    }
  });
}
