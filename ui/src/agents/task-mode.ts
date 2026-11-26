// How the person last started a task in each project ("One agent, here",
// "One agent, own copy", "Plan and split" or "Research") and with which agent,
// kept in the page's storage so a reload keeps the choice.

import type { AgentKind, Capabilities, ProjectId } from "../lib/protocol";

export type TaskMode = "here" | "own" | "split" | "research";

const KEY = "ovra.task-mode.";
const AGENT_KEY = "ovra.task-agent.";
const MODES: TaskMode[] = ["here", "own", "split", "research"];
const AGENTS: AgentKind[] = ["claude", "gemini", "codex"];

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Storage can be switched off; the form then starts on "One agent, here".
    return null;
  }
}

/** The mode last chosen in `project`, "here" when none was. */
export function rememberedMode(project: ProjectId, store = storage()): TaskMode {
  try {
    const kept = store?.getItem(KEY + project);
    return MODES.find((m) => m === kept) ?? "here";
  } catch {
    return "here";
  }
}

/** Keeps `mode` as the one to offer next time in `project`. */
export function rememberMode(project: ProjectId, mode: TaskMode, store = storage()): void {
  try {
    store?.setItem(KEY + project, mode);
  } catch {
    // A full or blocked storage only costs the choice after a reload.
  }
}

/** The agent last chosen in `project`, `claude` when none was. */
export function rememberedAgent(project: ProjectId, store = storage()): AgentKind {
  try {
    const kept = store?.getItem(AGENT_KEY + project);
    return AGENTS.find((a) => a === kept) ?? "claude";
  } catch {
    return "claude";
  }
}

/** Keeps `agent` as the one to offer next time in `project`. */
export function rememberAgent(project: ProjectId, agent: AgentKind, store = storage()): void {
  try {
    store?.setItem(AGENT_KEY + project, agent);
  } catch {
    // As for the mode.
  }
}

/**
 * The modes an agent with `can` may run in (ADR 0011): in this folder only
 * with an enforced policy, Plan and split only as a coordinator with IDE
 * tools, research only Claude's for now. Its own copy always, and only
 * that while another agent's capabilities are not known yet.
 */
export function modesFor(agent: AgentKind, can: Capabilities | undefined): TaskMode[] {
  if (agent === "claude") return MODES;
  if (!can) return ["own"];
  return MODES.filter(
    (m) =>
      m === "own" || (m === "here" && can.enforced_policy) || (m === "split" && can.ide_tools && can.read_only_role),
  );
}
