// Turns agent events into notifications: permission requests need the
// person (one toast for all of them, with a count), a finished turn waits
// for them, a failure is an error. A task's
// sessions (its coordinator and workers) finish together: one notification
// when none of them works any more, never one per session while others run.

import { failedLabel } from "../agents/errors";
import { agentSessions, openReviews } from "../agents/sessions.svelte";
import { describeTool } from "../approvals/describe";
import type { AgentStatus, PermissionId, PermissionRequest, ProjectId, SessionId, Task } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Notifier } from "./notifier.svelte";

/** The key of the one toast for all waiting approvals. */
export const APPROVALS_KEY = "permissions";

/** Listens until the returned function is called. `review` shows one pending request. */
export function watchAgents(transport: Transport, notifier: Notifier, review: (id: PermissionId) => void): () => void {
  const sessions = agentSessions(transport);
  const last = new Map<SessionId, AgentStatus["state"]>();
  const name = (id: SessionId) => sessions.name(id);
  const state = (id: SessionId) => last.get(id) ?? sessions.byId.get(id)?.status.state;
  const busy = (id: SessionId) => ["starting", "working", "waiting_for_approval"].includes(state(id) ?? "");

  /**
   * The task `session` belongs to, asked of the core: the roles known here may lag behind a spawn.
   * One agent in its own copy has no coordinator and is told about like any one session.
   */
  async function taskOf(
    project: ProjectId,
    session: SessionId,
  ): Promise<(Task & { coordinator: SessionId }) | undefined> {
    try {
      const { data } = await transport.call({ method: "orchestration.list", params: { project } });
      return data.find(
        (t): t is Task & { coordinator: SessionId } =>
          t.coordinator !== null && (t.coordinator === session || t.workers.some((w) => w.session === session)),
      );
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
      if (!sessions.transcripts.has(session)) await sessions.loadTranscript(session).catch(() => {});
      // It may have started again, or failed, meanwhile.
      if (state(session) !== "idle") return;
      // A turn that ended in an error is not done, though claude ends it like any other.
      const info = sessions.byId.get(session);
      const transcript = sessions.transcripts.get(session);
      if (info && failedLabel(info.status, transcript) !== null) {
        const error = [...(transcript ?? [])].reverse().find((m) => m.kind === "error");
        notifier.notify(
          {
            key: `session:${session}`,
            level: "error",
            text: `${name(session)} stopped${error?.kind === "error" ? `: ${error.message}` : "."}`,
          },
          "Agent stopped",
        );
        return;
      }
      const changed = sessions.changes(session).length;
      notifier.notify({
        key: `session:${session}`,
        level: "info",
        ...(changed > 0
          ? {
              text: `${name(session)} is done: ${changed === 1 ? "1 file" : `${changed} files`} changed.`,
              action: { label: "Review", run: () => openReviews([session]) },
              review: true,
            }
          : { text: `${name(session)} is done.` }),
      });
      return;
    }
    const members = [task.coordinator, ...task.workers.map((w) => w.session)];
    // A worker whose setup failed waits for Retry or Start anyway: the task waits on the person.
    const setupFailed = task.workers.filter((w) => state(w.session) === "preparing" && w.setup?.failed);
    const stuck = new Set(setupFailed.map((w) => w.session));
    // A worker waiting for a free agent, or being prepared, has not started: the task is not done.
    const unfinished = (id: SessionId) =>
      busy(id) || state(id) === "queued" || (state(id) === "preparing" && !stuck.has(id));
    if (members.some(unfinished)) return;
    await sessions.loadReviews(project);
    if (members.some(unfinished)) return;
    const title = sessions.byId.get(task.coordinator)?.title || "A task";
    if (setupFailed.length > 0) {
      const which = setupFailed.length === 1 ? (setupFailed[0]?.name ?? "") : `${setupFailed.length} workers`;
      notifier.notify(
        { key: `task:${task.id}`, level: "attention", text: `${title}: setup failed for ${which}` },
        "Setup failed",
      );
      return;
    }
    const waiting = sessions.waitingInTask(task.coordinator);
    notifier.notify({
      key: `task:${task.id}`,
      level: "info",
      ...(waiting > 0
        ? {
            text: `${title}: ${waiting === 1 ? "1 change" : `${waiting} changes`} to review`,
            action: { label: "Review", run: () => openReviews(task.workers.map((w) => w.session)) },
            review: true,
          }
        : { text: `${name(task.coordinator)} is done.` }),
    });
  }

  /** Requests still waiting, oldest first: one answered while its name loads gets no toast. */
  const pending = new Map<PermissionId, PermissionRequest>();

  /** The one approvals toast: the request itself while it is alone, else how many wait. */
  function approvalsToast(system: boolean): void {
    const [first] = pending.values();
    if (!first) {
      notifier.dismissKey(APPROVALS_KEY);
      return;
    }
    const text =
      pending.size === 1
        ? `${name(first.session)} needs approval: ${describeTool(first.tool, first.input, first.call).action}`
        : `${pending.size} requests need approval`;
    notifier.notify(
      { key: APPROVALS_KEY, level: "attention", text, action: { label: "Review", run: () => review(first.id) } },
      "Approval needed",
      system,
    );
  }

  async function requested(request: PermissionRequest): Promise<void> {
    pending.set(request.id, request);
    // A session that just started may not have its title or task yet.
    await sessions.named(request.project, request.session);
    if (!pending.has(request.id)) return;
    approvalsToast(true);
  }

  return transport.onEvent((event) => {
    switch (event.type) {
      case "permissions.requested":
        void requested(event.data);
        break;
      case "permissions.resolved":
        if (pending.delete(event.data.id)) approvalsToast(false);
        break;
      case "agents.status": {
        const { session, project, status } = event.data;
        const before = last.get(session);
        last.set(session, status.state);
        const task = sessions.roles.get(session)?.task;
        // Work again (or ask again): an earlier "done" or "stopped" is no longer true.
        if (busy(session)) {
          notifier.dismissKey(`session:${session}`);
          if (task) notifier.dismissKey(`task:${task}`);
        }
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
