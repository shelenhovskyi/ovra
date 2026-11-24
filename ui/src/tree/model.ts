// The file tree as data: loaded directory listings, which directories are
// expanded, and the flat list of visible rows the view renders a window of.
// Plain TypeScript (no runes) so a 100k-entry tree stays cheap: the view keeps
// `rows` in `$state.raw` and replaces it on every change.

import type { DirEntry, EntryKind, GitStatus } from "../lib/protocol";

export type Row = {
  path: string;
  name: string;
  depth: number;
  kind: EntryKind;
  git: GitStatus | null;
  ignored: boolean;
  /** Only meaningful for directories. */
  expanded: boolean;
  /** An expanded directory whose listing is still loading. */
  loading: boolean;
};

/** Loads the direct children of a directory (`""` is the root). Rejects with a CoreError. */
export type LoadDir = (path: string) => Promise<DirEntry[]>;

export function joinPath(dir: string, name: string): string {
  return dir === "" ? name : `${dir}/${name}`;
}

export function parentOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash < 0 ? "" : path.slice(0, slash);
}

function isInside(path: string, dir: string): boolean {
  return dir === "" || path === dir || path.startsWith(`${dir}/`);
}

export class TreeModel {
  rows: Row[] = [];
  /** The last load failure, shown by the view; cleared by the next successful load. */
  error: string | null = null;

  private readonly dirs = new Map<string, DirEntry[]>();
  private readonly expanded = new Set<string>([""]);
  private readonly loading = new Set<string>();

  constructor(
    private readonly load: LoadDir,
    private readonly onChange: () => void,
  ) {}

  /** Loads the root listing. */
  start(): Promise<void> {
    return this.fetch("");
  }

  isLoading(path: string): boolean {
    return this.loading.has(path);
  }

  isExpanded(path: string): boolean {
    return this.expanded.has(path);
  }

  async expand(path: string): Promise<void> {
    if (this.expanded.has(path)) return;
    this.expanded.add(path);
    this.rebuild();
    if (!this.dirs.has(path)) await this.fetch(path);
  }

  collapse(path: string): void {
    if (path === "" || !this.expanded.delete(path)) return;
    this.rebuild();
  }

  toggle(path: string): Promise<void> {
    if (this.expanded.has(path)) {
      this.collapse(path);
      return Promise.resolve();
    }
    return this.expand(path);
  }

  /** Reloads the loaded directories that contain or are any of `paths` (an `fs.changed` event). */
  async refreshPaths(paths: string[]): Promise<void> {
    const targets = new Set<string>();
    for (const path of paths) {
      for (const dir of [path, parentOf(path)]) if (this.dirs.has(dir)) targets.add(dir);
    }
    await Promise.all([...targets].map((dir) => this.fetch(dir)));
  }

  /** Reloads every loaded directory, for fresh git status (an `fs.git_changed` event). */
  async refreshAll(): Promise<void> {
    await Promise.all([...this.dirs.keys()].map((dir) => this.fetch(dir)));
  }

  private async fetch(dir: string): Promise<void> {
    if (this.loading.has(dir)) return;
    this.loading.add(dir);
    this.rebuild();
    try {
      const entries = await this.load(dir);
      this.forgetMissingChildren(dir, entries);
      this.dirs.set(dir, entries);
      this.error = null;
    } catch (err) {
      const code = (err as { code?: string }).code;
      if (code === "not_found" && dir !== "") this.forget(dir);
      else this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading.delete(dir);
      this.rebuild();
    }
  }

  /** Drops state for child directories that are gone from a fresh listing of `dir`. */
  private forgetMissingChildren(dir: string, entries: DirEntry[]): void {
    const old = this.dirs.get(dir);
    if (!old) return;
    const present = new Set(entries.filter((e) => e.kind === "dir").map((e) => e.name));
    for (const entry of old) {
      if (entry.kind === "dir" && !present.has(entry.name)) this.forget(joinPath(dir, entry.name));
    }
  }

  private forget(dir: string): void {
    for (const key of [...this.dirs.keys()]) if (isInside(key, dir)) this.dirs.delete(key);
    for (const key of [...this.expanded]) if (isInside(key, dir)) this.expanded.delete(key);
  }

  private rebuild(): void {
    const rows: Row[] = [];
    const walk = (dir: string, depth: number, ignoredDir: boolean): void => {
      for (const entry of this.dirs.get(dir) ?? []) {
        const path = joinPath(dir, entry.name);
        const isDir = entry.kind === "dir";
        const expanded = isDir && this.expanded.has(path);
        rows.push({
          path,
          name: entry.name,
          depth,
          kind: entry.kind,
          git: entry.git,
          ignored: ignoredDir || entry.ignored,
          expanded,
          loading: expanded && this.loading.has(path),
        });
        if (expanded) walk(path, depth + 1, ignoredDir || entry.ignored);
      }
    };
    walk("", 0, false);
    this.rows = rows;
    this.onChange();
  }
}

/** Which rows a viewport of `height` px scrolled to `scrollTop` shows, plus `overscan` rows each side. */
export function visibleRange(
  scrollTop: number,
  height: number,
  rowHeight: number,
  count: number,
  overscan: number,
): { start: number; end: number } {
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(count, Math.ceil((scrollTop + height) / rowHeight) + overscan);
  return { start, end: Math.max(start, end) };
}
