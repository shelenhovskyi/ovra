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

/** Every project's terminal tabs, in one entry; an unreadable entry is treated as none. */
export function tabStore(store = storage()): TabStore {
  const read = (): Record<string, unknown> => {
    try {
      const all: unknown = JSON.parse(store?.getItem(TABS) ?? "{}");
      return typeof all === "object" && all !== null && !Array.isArray(all) ? (all as Record<string, unknown>) : {};
    } catch {
      return {};
    }
  };
  return {
    load(project) {
      const saved = read()[project] as Partial<SavedTabs> | undefined;
      if (!saved || !Array.isArray(saved.tabs) || !saved.tabs.every(isSavedTab)) return null;
      return { tabs: saved.tabs, active: typeof saved.active === "number" ? saved.active : 0 };
    },
    save(project, tabs) {
      const all = read();
      if (tabs) all[project] = tabs;
      else delete all[project];
      try {
        if (Object.keys(all).length === 0) store?.removeItem(TABS);
        else store?.setItem(TABS, JSON.stringify(all));
      } catch {
        // A full or blocked storage only costs the restore.
      }
    },
  };
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
