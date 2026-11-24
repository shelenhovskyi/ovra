// Unsaved text kept in the browser's storage, so a reload or a closed window
// never loses it ("hot exit"). A draft remembers the file's mtime when it
// was loaded; if the file changed on disk since, the editor shows a conflict
// instead of picking one version silently.

import { SvelteSet } from "svelte/reactivity";
import type { ProjectId } from "../lib/protocol";

export type Draft = { text: string; mtime: number };

const PREFIX = "ovra.draft.";

export class Drafts {
  /** Keys of stored drafts; reactive. */
  private readonly keys = new SvelteSet<string>();

  constructor(private readonly storage: Storage | null) {
    try {
      for (let i = 0; storage && i < storage.length; i++) {
        const key = storage.key(i);
        if (key?.startsWith(PREFIX)) this.keys.add(key);
      }
    } catch {
      // Storage unavailable: drafts live as long as the page.
    }
  }

  has(project: ProjectId, path: string): boolean {
    return this.keys.has(key(project, path));
  }

  get(project: ProjectId, path: string): Draft | null {
    try {
      const raw = this.storage?.getItem(key(project, path));
      return raw ? (JSON.parse(raw) as Draft) : null;
    } catch {
      return null;
    }
  }

  /** Stores a draft; false if the storage refused it (quota, disabled). */
  set(project: ProjectId, path: string, draft: Draft): boolean {
    const k = key(project, path);
    try {
      if (!this.storage) return false;
      this.storage.setItem(k, JSON.stringify(draft));
      this.keys.add(k);
      return true;
    } catch {
      return false;
    }
  }

  remove(project: ProjectId, path: string): void {
    const k = key(project, path);
    this.keys.delete(k);
    try {
      this.storage?.removeItem(k);
    } catch {
      // Nothing to clean up.
    }
  }
}

function key(project: ProjectId, path: string): string {
  return `${PREFIX}${project}:${path}`;
}

function localStorageOrNull(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export const drafts = new Drafts(localStorageOrNull());
