// What a session's sandbox means, in plain words, for the session panel,
// the session list and the approval cards.

import type { SandboxReport, SessionInfo } from "../lib/protocol";
import { isEnded } from "./status";

export type SandboxState =
  /** The runtime has not said yet how it runs the current process. */
  | { kind: "pending" }
  | { kind: "none" }
  | { kind: "sandboxed"; report: SandboxReport }
  /** An ended session saved before Ovra recorded the report: nothing to say. */
  | { kind: "unknown" };

export function sandboxState(info: SessionInfo): SandboxState {
  if (info.sandbox) return { kind: "sandboxed", report: info.sandbox };
  if (info.sandbox_reported) return { kind: "none" };
  return isEnded(info.status) ? { kind: "unknown" } : { kind: "pending" };
}

const NETWORK: Record<SandboxReport["network"], string> = {
  proxy: "network only through Ovra's proxy",
  blocked: "no network",
  open: "network not limited",
};

/** One line for the session detail; `warning` lines use the warning style. */
export function sandboxLine(state: SandboxState): { text: string; warning: boolean } | null {
  switch (state.kind) {
    case "sandboxed":
      return { text: `Runs in a sandbox (${state.report.name}), ${NETWORK[state.report.network]}.`, warning: false };
    case "none":
      return { text: "No sandbox: the agent can read and change anything your user can.", warning: true };
    case "pending":
      return { text: "Sandbox: starting…", warning: false };
    case "unknown":
      return null;
  }
}

/** A word for the session list, where there is no room for a sentence. */
export function sandboxBadge(state: SandboxState): string | null {
  if (state.kind === "sandboxed") return "Sandboxed";
  return state.kind === "none" ? "No sandbox" : null;
}
