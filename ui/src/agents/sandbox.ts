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

/** The one wording for a session without a sandbox, in the list and the session alike. */
export const NO_SANDBOX = "No sandbox: the agent can change anything you can";

/** What a sandbox lets through, after "Sandboxed · " on the short line. */
const NETWORK_SHORT: Record<SandboxReport["network"], string> = {
  proxy: "",
  blocked: "no network",
  open: "network not limited",
};

/**
 * One short line for the session detail, and the whole sentence (`more`,
 * in the past tense once it `ended`) for its Details; `warning` lines are
 * security facts that stand out.
 */
export function sandboxLine(
  state: SandboxState,
  ended: boolean,
): { text: string; warning: boolean; more: string | null } | null {
  switch (state.kind) {
    case "sandboxed": {
      const { name, network, gaps } = state.report;
      const text = ["Sandboxed", gaps.length > 0 ? "gaps" : "", NETWORK_SHORT[network]].filter(Boolean).join(" · ");
      const more = `${ended ? "Ran" : "Runs"} in a sandbox (${name}), ${NETWORK[network]}.`;
      return { text, warning: network === "open", more };
    }
    case "none":
      return { text: NO_SANDBOX, warning: true, more: null };
    case "pending":
      return { text: "Sandbox: starting…", warning: false, more: null };
    case "unknown":
      return null;
  }
}

/** What the session's sandbox does not cover, for the line under the
 * sandbox line, in the past tense once it `ended`; null when it reports no
 * gaps or there is no sandbox. */
export function sandboxGaps(state: SandboxState, ended: boolean): string | null {
  if (state.kind !== "sandboxed" || state.report.gaps.length === 0) return null;
  return `${ended ? "Did not cover" : "Not covered"}: ${state.report.gaps.join(", ")}.`;
}

/**
 * One warning line for the whole task list, said once instead of on every
 * row: whether any session that still runs has no sandbox, or an open network.
 */
export function listWarning(sessions: SessionInfo[]): string | null {
  const states = sessions.filter((s) => !isEnded(s.status)).map(sandboxState);
  if (states.some((s) => s.kind === "none")) return NO_SANDBOX;
  if (states.some((s) => s.kind === "sandboxed" && s.report.network === "open"))
    return "Sandboxed, network not limited";
  return null;
}
