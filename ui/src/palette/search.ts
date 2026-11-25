// "Search or run": what an empty query shows, synonyms, and what was chosen lately.

import type { Entry } from "./sources";

const KEY = "ovra.palette.recent";
const RECENT_MAX = 5;

export function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** Ids of the entries chosen last, newest first. */
export function recentIds(store = storage()): string[] {
  try {
    const saved: unknown = JSON.parse(store?.getItem(KEY) ?? "[]");
    return Array.isArray(saved) ? saved.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

export function noteRecent(id: string, store = storage()): void {
  const ids = [id, ...recentIds(store).filter((other) => other !== id)].slice(0, RECENT_MAX * 4);
  try {
    store?.setItem(KEY, JSON.stringify(ids));
  } catch {
    // A full or blocked storage only costs the list.
  }
}

/**
 * The rows of an empty query: up to five recent entries that still exist,
 * then the `suggested` ones, or (`null`, no project) the Projects group.
 */
export function idleRows(entries: Entry[], recent: string[], suggested: string[] | null): Entry[] {
  const byId = new Map(entries.map((e) => [e.id, e]));
  const shown = recent.flatMap((id) => byId.get(id) ?? []).slice(0, RECENT_MAX);
  const taken = new Set(shown.map((e) => e.id));
  const rest = (
    suggested === null
      ? entries.filter((e) => e.group === "Projects")
      : suggested.flatMap((id) => byId.get(id) ?? []).map((e) => ({ ...e, group: "Suggested" }))
  ).filter((e) => !taken.has(e.id));
  return [...shown.map((e) => ({ ...e, group: "Recent" })), ...rest];
}

/** Entries not in `taken` with a keyword word that starts with `query`: "proxy" finds Network. */
export function keywordMatches(entries: Entry[], query: string, taken: Set<string>): Entry[] {
  const words = query.trim().toLowerCase();
  if (!words) return [];
  return entries.filter(
    (e) => !taken.has(e.id) && e.keywords?.some((k) => ` ${k.toLowerCase()}`.includes(` ${words}`)),
  );
}
