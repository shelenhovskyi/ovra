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

/** One line for the session detail, in the past tense once it `ended`;
 * `warning` lines use the warning style. */
export function sandboxLine(state: SandboxState, ended: boolean): { text: string; warning: boolean } | null {
  switch (state.kind) {
    case "sandboxed": {
      const { name, network } = state.report;
      const verb = ended ? "Ran" : "Runs";
      return { text: `${verb} in a sandbox (${name}), ${NETWORK[network]}.`, warning: network === "open" };
    }
    case "none":
      return ended
        ? { text: "Ran without a sandbox: the agent could read and change anything your user can.", warning: true }
        : { text: "No sandbox: the agent can read and change anything your user can.", warning: true };
    case "pending":
      return { text: "Sandbox: starting…", warning: false };
    case "unknown":
      return null;
  }
}

/** A word or two for the session list, where there is no room for a sentence. */
export function sandboxBadge(state: SandboxState): { text: string; warning: boolean } | null {
  if (state.kind === "none") return { text: "No sandbox", warning: true };
  if (state.kind !== "sandboxed") return null;
  return state.report.network === "open"
    ? { text: "Sandboxed, open network", warning: true }
    : { text: "Sandboxed", warning: false };
}
