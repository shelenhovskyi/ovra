// How the person last started a task in each project ("One agent, here",
// "Plan and split" or "Research"), kept in the page's storage so a reload keeps the choice.

import type { ProjectId } from "../lib/protocol";

export type TaskMode = "here" | "split" | "research";

const KEY = "ovra.task-mode.";

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
    return kept === "split" || kept === "research" ? kept : "here";
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
