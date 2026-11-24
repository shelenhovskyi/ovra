// Turns agent events into notifications: a permission request needs the
// person, a finished turn waits for them, a failure is an error.

import { agentSessions, sessionName } from "../agents/sessions.svelte";
import { describeTool } from "../approvals/describe";
import type { AgentStatus, PermissionId, SessionId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import type { Notifier } from "./notifier.svelte";

/** Listens until the returned function is called. `review` shows one pending request. */
export function watchAgents(transport: Transport, notifier: Notifier, review: (id: PermissionId) => void): () => void {
  const sessions = agentSessions(transport);
  const last = new Map<SessionId, AgentStatus["state"]>();
  const name = (id: SessionId) => sessionName(sessions.byId.get(id), id);
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
        const { session, status } = event.data;
        const before = last.get(session);
        last.set(session, status.state);
        if (status.state === "idle" && (before === "working" || before === "waiting_for_approval")) {
          notifier.notify({
            key: `session:${session}`,
            level: "info",
            text: `${name(session)} finished and waits for you`,
          });
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
