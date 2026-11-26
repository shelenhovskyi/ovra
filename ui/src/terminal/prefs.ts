// Terminal settings kept in the page's storage, so they survive a reload:
// screen reader mode, whether the panel is shown, and each project's tabs.

import type { SavedTab, SavedTabs, TabStore } from "./sessions.svelte";

const SCREEN_READER = "ovra.terminal.screenReader";

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Storage can be switched off; the setting then lasts until the window closes.
    return null;
  }
}

export function loadScreenReader(store = storage()): boolean {
  try {
    return store?.getItem(SCREEN_READER) === "on";
  } catch {
    return false;
  }
}

export function saveScreenReader(on: boolean, store = storage()): void {
  try {
    if (on) store?.setItem(SCREEN_READER, "on");
    else store?.removeItem(SCREEN_READER);
  } catch {
    // A full or blocked storage only costs the setting after a reload.
  }
}

const TABS = "ovra.terminal.tabs";
const OPEN = "ovra.terminal.open";

function isSavedTab(value: unknown): value is SavedTab {
  if (typeof value !== "object" || value === null) return false;
  const tab = value as Record<string, unknown>;
  return (
    (tab.id === null || typeof tab.id === "string") &&
    typeof tab.title === "string" &&
    (tab.cwd === undefined || typeof tab.cwd === "string")
  );
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** One window's tabs; `open` is false once the window was left, so another may take them. */
type WindowTabs = { open: boolean; at: number; projects: Record<string, unknown> };

const WINDOW = "ovra.terminal.window";
/** Windows kept at most; the least recently saved go first (windows that never said they left). */
const MAX_WINDOWS = 8;

function sessionStorageOrNull(): Storage | null {
  try {
    return typeof sessionStorage === "undefined" ? null : sessionStorage;
  } catch {
    return null;
  }
}

function newWindowId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * Every project's terminal tabs, per window, so two windows never take the
 * same shells or overwrite each other's tabs. A window's id lives in its
 * session storage and survives a reload; a new window takes over the tabs of
 * the window left last. An unreadable entry is treated as none.
 */
export function tabStore(store = storage(), session = sessionStorageOrNull(), now = Date.now) {
  const read = (): Record<string, WindowTabs> => {
    try {
      const all: unknown = JSON.parse(store?.getItem(TABS) ?? "{}");
      if (!isObject(all)) return {};
      const windows: Record<string, WindowTabs> = {};
      const before: Record<string, unknown> = {};
      for (const [id, entry] of Object.entries(all)) {
        if (isObject(entry) && isObject(entry.projects) && typeof entry.at === "number") {
          windows[id] = { open: entry.open === true, at: entry.at, projects: entry.projects };
        } else if (isObject(entry) && Array.isArray(entry.tabs)) {
          before[id] = entry; // saved before tabs were kept per window
        }
      }
      if (Object.keys(before).length > 0) windows.before = { open: false, at: 0, projects: before };
      return windows;
    } catch {
      return {};
    }
  };
  const write = (all: Record<string, WindowTabs>): void => {
    const ids = Object.keys(all).sort((a, b) => (all[b]?.at ?? 0) - (all[a]?.at ?? 0));
    for (const id of ids.slice(MAX_WINDOWS)) delete all[id];
    try {
      if (Object.keys(all).length === 0) store?.removeItem(TABS);
      else store?.setItem(TABS, JSON.stringify(all));
    } catch {
      // A full or blocked storage only costs the restore.
    }
  };

  let me: string | null = null;
  /** This window's entry: its own after a reload, else the one left last, else a new one. */
  const claim = (all: Record<string, WindowTabs>): WindowTabs => {
    if (me === null) {
      let id: string | null = null;
      try {
        id = session?.getItem(WINDOW) ?? null;
      } catch {
        // Without session storage each load is a new window.
      }
      // Still open: a duplicated tab copied the id, so this is another window.
      if (id === null || all[id]?.open) {
        id = newWindowId();
        const left = Object.entries(all)
          .filter(([, entry]) => !entry.open)
          .sort(([, a], [, b]) => b.at - a.at)[0];
        if (left) {
          all[id] = left[1];
          delete all[left[0]];
        }
        try {
          session?.setItem(WINDOW, id);
        } catch {
          // Only costs taking the tabs back after a reload.
        }
      }
      me = id;
    }
    const entry = all[me] ?? { open: true, at: now(), projects: {} };
    entry.open = true;
    all[me] = entry;
    return entry;
  };

  return {
    load(project: string): SavedTabs | null {
      const all = read();
      const saved = claim(all).projects[project] as Partial<SavedTabs> | undefined;
      write(all);
      if (!saved || !Array.isArray(saved.tabs) || !saved.tabs.every(isSavedTab)) return null;
      return { tabs: saved.tabs, active: typeof saved.active === "number" ? saved.active : 0 };
    },
    save(project: string, tabs: SavedTabs | null): void {
      const all = read();
      const entry = claim(all);
      if (tabs) entry.projects[project] = tabs;
      else delete entry.projects[project];
      entry.at = now();
      write(all);
    },
    /** The window is being left (closed or reloaded): its tabs may be taken by the next window. */
    leave(): void {
      if (me === null) return;
      const all = read();
      const entry = all[me];
      if (!entry) return;
      if (Object.keys(entry.projects).length === 0) delete all[me];
      else Object.assign(entry, { open: false, at: now() });
      write(all);
    },
  } satisfies TabStore & { leave(): void };
}

/** Whether the terminal panel was shown when the page was left. */
export function loadOpen(store = storage()): boolean {
  try {
    return store?.getItem(OPEN) === "on";
  } catch {
    return false;
  }
}

export function saveOpen(open: boolean, store = storage()): void {
  try {
    if (open) store?.setItem(OPEN, "on");
    else store?.removeItem(OPEN);
  } catch {
    // Only costs showing the panel after a reload.
  }
}
