// Words for agent states. Every state has a text label, so state is never
// shown by colour alone.

import type { AgentStatus } from "../lib/protocol";

export function statusLabel(status: AgentStatus): string {
  switch (status.state) {
    case "queued":
      return "Waiting for a free agent";
    case "starting":
      return "Starting";
    case "working":
      return "Working";
    case "waiting_for_approval":
      return "Needs approval";
    case "idle":
      // The turn ended and nothing waits for an answer: done, until the person
      // asks for more. A turn that ended with an error says so instead
      // (`failedLabel`).
      return "Done";
    case "stopped":
      return "Stopped";
    case "failed":
      return `Failed: ${status.message}`;
  }
}

/** Whether the session can no longer take messages. */
export function isEnded(status: AgentStatus): boolean {
  return status.state === "stopped" || status.state === "failed";
}

/** Whether the session is starting or in a turn. */
export function isBusy(status: AgentStatus | null): boolean {
  return status?.state === "starting" || status?.state === "working" || status?.state === "waiting_for_approval";
}
