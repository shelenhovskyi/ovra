// Words for agent states. Every state has a text label, so state is never
// shown by colour alone.

import type { AgentStatus } from "../lib/protocol";

export function statusLabel(status: AgentStatus): string {
  switch (status.state) {
    case "starting":
      return "Starting";
    case "working":
      return "Working";
    case "waiting_for_approval":
      return "Needs approval";
    case "idle":
      return "Waiting for you";
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
