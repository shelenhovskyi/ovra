// The files opened lately in each project, newest first, kept in localStorage
// per project id: "Go to file" lists them first on an empty query.

import type { ProjectId } from "../lib/protocol";
import type { Workspace } from "../shell/workspace.svelte";
import { storage } from "./search";
import type { Entry } from "./sources";

const RECENT_FILES_MAX = 20;

const key = (project: ProjectId) => `ovra.palette.recent_files:${project}`;

/** Paths of the files opened last in `project`, newest first. */
export function recentFiles(project: ProjectId, store = storage()): string[] {
  try {
    const saved: unknown = JSON.parse(store?.getItem(key(project)) ?? "[]");
    return Array.isArray(saved) ? saved.filter((path): path is string => typeof path === "string") : [];
  } catch {
    return [];
  }
}

function save(project: ProjectId, paths: string[], store: Storage | null): void {
  try {
    store?.setItem(key(project), JSON.stringify(paths.slice(0, RECENT_FILES_MAX)));
  } catch {
    // A full or blocked storage only costs the list.
  }
}

export function noteRecentFile(project: ProjectId, path: string, store = storage()): void {
  save(project, [path, ...recentFiles(project, store).filter((other) => other !== path)], store);
}

/**
 * The file entries of `project` opened lately, newest first, without `current`
 * (the file shown now). Paths missing from `entries` no longer exist and are
 * dropped from the saved list.
 */
export function recentFileEntries(
  project: ProjectId,
  entries: Entry[],
  current: string | null,
  store = storage(),
): Entry[] {
  const byPath = new Map(entries.map((e) => [e.label, e]));
  const saved = recentFiles(project, store);
  const kept = saved.filter((path) => byPath.has(path));
  if (kept.length < saved.length) save(project, kept, store);
  return kept.filter((path) => path !== current).flatMap((path) => byPath.get(path) ?? []);
}

/**
 * An empty "Go to file": the recent files, then the first `limit` of the
 * others; null, to rank as usual, when nothing was opened lately.
 */
export function recentFirst(recent: Entry[], entries: Entry[], limit: number): Entry[] | null {
  if (recent.length === 0) return null;
  const taken = new Set(recent.map((e) => e.id));
  const rest = entries.filter((e) => !taken.has(e.id)).slice(0, limit);
  return [...recent.map((e) => ({ ...e, group: "Recent" })), ...rest.map((e) => ({ ...e, group: "Files" }))];
}

/** Notes each file the window shows in its project's list; returns a function that stops. */
export function trackRecentFiles(workspace: Pick<Workspace, "project" | "openPath">, store = storage()): () => void {
  return $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      const path = workspace.openPath;
      if (project && path) noteRecentFile(project, path, store);
    });
  });
}
