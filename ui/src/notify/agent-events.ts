// Turns agent events into notifications: a permission request needs the
// person, a finished turn waits for them, a failure is an error. A task's
// sessions (its coordinator and workers) finish together: one notification
// when none of them works any more, never one per session while others run.

import { agentSessions, openReviews } from "../agents/sessions.svelte";
import { describeTool } from "../approvals/describe";
import type { AgentStatus, PermissionId, PermissionRequest, ProjectId, SessionId, Task } from "../lib/protocol";
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
      if (state(session) !== "idle") return;
      // Its title may come with the list; what it changed, with its review.
      await sessions.named(project, session);
      await sessions.loadReviews(project);
      // It may have started again, or failed, meanwhile.
      if (state(session) !== "idle") return;
      const changed = sessions.changes(session).length;
      notifier.notify({
        key: `session:${session}`,
        level: "info",
        ...(changed > 0
          ? {
              text: `${name(session)} is done and changed ${changed === 1 ? "1 file" : `${changed} files`}. Keep or undo it in Reviews.`,
              action: { label: "Review", run: openReviews },
            }
          : { text: `${name(session)} is done. Reply to ask for more.` }),
      });
      return;
    }
    const members = [task.coordinator, ...task.workers.map((w) => w.session)];
    // A worker waiting for a free agent has not started: the task is not done.
    const unfinished = (id: SessionId) => busy(id) || state(id) === "queued";
    if (members.some(unfinished)) return;
    await sessions.loadReviews(project);
    if (members.some(unfinished)) return;
    const waiting = sessions.waitingInTask(task.coordinator);
    notifier.notify({
      key: `task:${task.id}`,
      level: "info",
      ...(waiting > 0
        ? {
            text: `${sessions.byId.get(task.coordinator)?.title || "A task"}: ${waiting === 1 ? "1 change waits" : `${waiting} changes wait`} for your review`,
            action: { label: "Review", run: openReviews },
          }
        : { text: `${name(task.coordinator)} is done. Reply to ask for more.` }),
    });
  }

  /** Requests still waiting: one answered while its name loads gets no toast. */
  const pending = new Set<PermissionId>();

  async function requested(request: PermissionRequest): Promise<void> {
    pending.add(request.id);
    // A session that just started may not have its title or task yet.
    await sessions.named(request.project, request.session);
    if (!pending.has(request.id)) return;
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
  }

  return transport.onEvent((event) => {
    switch (event.type) {
      case "permissions.requested":
        void requested(event.data);
        break;
      case "permissions.resolved":
        pending.delete(event.data.id);
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
