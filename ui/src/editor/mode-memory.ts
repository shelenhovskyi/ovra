// The last Edit / View choice per kind of file, kept in the browser's storage
// so a file without a tab opens the way its kind was last shown: Markdown in
// View if that was the last choice, TypeScript in Edit. Storage errors only
// mean the choice is forgotten.

import type { MainMode } from "../shell/workspace.svelte";

const KEY = "ovra.editor.modes";

type Store = Pick<Storage, "getItem" | "setItem">;

/** `.md` for `docs/README.MD`; a file without an extension is its own kind (`Makefile`). */
export function kindOf(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot) : name;
}

export class ModeMemory {
  private modes: Record<string, MainMode> = {};

  constructor(private readonly storage: Store | null) {
    try {
      const raw = storage?.getItem(KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : null;
      if (parsed && typeof parsed === "object") {
        for (const [kind, mode] of Object.entries(parsed)) {
          if (mode === "edit" || mode === "view") this.modes[kind] = mode;
        }
      }
    } catch {
      // Unreadable or unavailable: start with no choices.
    }
  }

  /** The mode files like `path` were last switched to, if any. */
  get(path: string): MainMode | undefined {
    return this.modes[kindOf(path)];
  }

  set(path: string, mode: MainMode): void {
    this.modes[kindOf(path)] = mode;
    try {
      this.storage?.setItem(KEY, JSON.stringify(this.modes));
    } catch {
      // Quota or disabled storage: the choice lasts as long as the page.
    }
  }
}

function localStorageOrNull(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export const modeMemory = new ModeMemory(localStorageOrNull());
