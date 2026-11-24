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
  /** For `git: "renamed"`, the old project-relative path, when the core knows it. */
  renamedFrom: string | null;
  ignored: boolean;
  /** Only meaningful for directories. */
  expanded: boolean;
  /** An expanded directory whose first listing is still loading. */
  loading: boolean;
  /** Position among its siblings (from 1) and their count, for `aria-posinset` / `aria-setsize`. */
  posinset: number;
  setsize: number;
};

export type Filter = "all" | "changed";

/** Loads the direct children of a directory (`""` is the root). Rejects with a CoreError. */
export type LoadDir = (path: string) => Promise<DirEntry[]>;

export function joinPath(dir: string, name: string): string {
  return dir === "" ? name : `${dir}/${name}`;
}

export function parentOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash < 0 ? "" : path.slice(0, slash);
}

/** The directories that contain `path`, outermost first, without the root. */
export function ancestorsOf(path: string): string[] {
  const parts = path.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

function isInside(path: string, dir: string): boolean {
  return dir === "" || path === dir || path.startsWith(`${dir}/`);
}

// Natural order, so `module-2` comes before `module-10`; case does not split the list.
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Directories first, then names in natural order; `.git` is never shown. */
export function sortEntries(entries: DirEntry[]): DirEntry[] {
  return entries
    .filter((e) => e.name !== ".git")
    .sort((a, b) => Number(b.kind === "dir") - Number(a.kind === "dir") || collator.compare(a.name, b.name));
}

export class TreeModel {
  rows: Row[] = [];
  /** The last load failure, shown by the view; cleared by the next successful load. */
  error: string | null = null;
  filter: Filter = "all";

  private readonly dirs = new Map<string, DirEntry[]>();
  private expanded = new Set<string>([""]);
  private readonly loading = new Set<string>();
  /** Loaded directories whose listing may be out of date; reloaded when shown. */
  private readonly stale = new Set<string>();
  private readonly ignoredDirs = new Set<string>();
  /** Directories whose rows the view shows; null means all of them. */
  private view: Set<string> | null = null;
  private pumping: Promise<void> | null = null;
  /** Expanded directories before the "changed" filter opened more. */
  private expandedBeforeFilter: Set<string> | null = null;

  constructor(
    private readonly load: LoadDir,
    private readonly onChange: () => void,
  ) {}

  /** Loads the root listing, and the listings of `expanded` (restored from the last session). */
  async start(expanded: string[] = []): Promise<void> {
    for (const dir of expanded) if (dir !== "") this.expanded.add(dir);
    await Promise.all([...this.expanded].map((d) => this.fetchFirst(d)));
  }

  /** Expanded directories, outermost first, for saving the tree's shape. */
  expandedDirs(): string[] {
    return [...(this.expandedBeforeFilter ?? this.expanded)].filter((d) => d !== "").sort();
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
    if (!this.dirs.has(path)) await this.fetchFirst(path);
    else await this.pump();
  }

  collapse(path: string): void {
    if (path === "" || !this.expanded.delete(path)) return;
    this.rebuild();
  }

  collapseAll(): void {
    this.expanded = new Set([""]);
    this.expandedBeforeFilter = null;
    this.filter = "all";
    this.rebuild();
  }

  toggle(path: string): Promise<void> {
    if (this.expanded.has(path)) {
      this.collapse(path);
      return Promise.resolve();
    }
    return this.expand(path);
  }

  /** Expands every folder above `path` so its row is shown. */
  async reveal(path: string): Promise<void> {
    if (this.filter !== "all") this.setFilter("all");
    for (const dir of ancestorsOf(path)) await this.expand(dir);
  }

  /**
   * "changed" shows only entries with a git status and opens every folder that
   * has changes inside (except wholly untracked ones), so the cost follows the
   * number of changes, not the size of the tree.
   */
  async setFilter(filter: Filter): Promise<void> {
    if (filter === this.filter) return;
    this.filter = filter;
    if (filter === "all") {
      if (this.expandedBeforeFilter) this.expanded = this.expandedBeforeFilter;
      this.expandedBeforeFilter = null;
      this.rebuild();
      await this.pump();
      return;
    }
    this.expandedBeforeFilter = new Set(this.expanded);
    this.rebuild();
    await this.expandChanged();
  }

  private async expandChanged(): Promise<void> {
    for (;;) {
      // A wholly untracked folder can hold thousands of new files; it stays closed until asked.
      const closed = this.rows
        .filter((r) => r.kind === "dir" && r.git && r.git !== "untracked" && !r.expanded)
        .map((r) => r.path);
      if (closed.length === 0 || this.filter !== "changed") return;
      for (const dir of closed) this.expanded.add(dir);
      this.rebuild();
      await Promise.all(closed.map((dir) => (this.dirs.has(dir) ? undefined : this.fetchFirst(dir))));
      await this.pump();
    }
  }

  /** The directories whose rows are on screen; listings outside it refresh when they scroll in. */
  setView(dirs: Set<string> | null): Promise<void> {
    this.view = dirs;
    return this.pump();
  }

  /**
   * An `fs.changed` event: the listings of the changed paths' folders, and of
   * changed folders with everything loaded under them (the watcher reports
   * big batches as their top-level folders), are out of date.
   */
  invalidatePaths(paths: string[]): Promise<void> {
    for (const path of paths) {
      if (this.dirs.has(parentOf(path))) this.stale.add(parentOf(path));
      for (const dir of this.dirs.keys()) if (isInside(dir, path)) this.stale.add(dir);
    }
    return this.pump();
  }

  /** An `fs.git_changed` event: every loaded listing carries git status, so all may be out of date. */
  invalidateAll(): Promise<void> {
    for (const dir of this.dirs.keys()) this.stale.add(dir);
    return this.pump();
  }

  /** Ignored folders are not watched by the core; re-list the shown ones (on window focus). */
  invalidateIgnored(): Promise<void> {
    for (const dir of this.ignoredDirs) if (this.dirs.has(dir)) this.stale.add(dir);
    return this.pump();
  }

  private isShown(dir: string): boolean {
    for (let d = dir; d !== ""; d = parentOf(d)) if (!this.expanded.has(d)) return false;
    return true;
  }

  /** Reloads stale listings that are shown, in batches with one rebuild each, until none are left. */
  private pump(): Promise<void> {
    if (this.pumping) return this.pumping;
    const due = this.due();
    if (due.length === 0) return Promise.resolve();
    const run = async (batch: string[]): Promise<void> => {
      for (; batch.length > 0; batch = this.due()) {
        for (const dir of batch) this.stale.delete(dir);
        await Promise.all(batch.map((dir) => this.fetch(dir)));
        this.rebuild();
      }
    };
    this.pumping = run(due).finally(() => {
      this.pumping = null;
    });
    return this.pumping;
  }

  private due(): string[] {
    return [...this.stale].filter((d) => this.isShown(d) && (!this.view || this.view.has(d)));
  }

  /** First load of a directory: shows the loading hint while it runs. */
  private async fetchFirst(dir: string): Promise<void> {
    if (this.loading.has(dir)) return;
    this.loading.add(dir);
    this.rebuild();
    await this.fetch(dir);
    this.loading.delete(dir);
    this.rebuild();
  }

  private async fetch(dir: string): Promise<void> {
    try {
      const entries = sortEntries(await this.load(dir));
      this.forgetMissingChildren(dir, entries);
      this.dirs.set(dir, entries);
      this.error = null;
    } catch (err) {
      // A CoreError carries the core's message without its `code:` prefix.
      const core = err as { code?: string; error?: { message: string } };
      if (core.code === "not_found" && dir !== "") this.forget(dir);
      else this.error = core.error?.message ?? (err instanceof Error ? err.message : String(err));
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
    for (const key of [...this.stale]) if (isInside(key, dir)) this.stale.delete(key);
  }

  private rebuild(): void {
    const rows: Row[] = [];
    const changedOnly = this.filter === "changed";
    this.ignoredDirs.clear();
    const walk = (dir: string, depth: number, ignoredDir: boolean): void => {
      const entries = (this.dirs.get(dir) ?? []).filter((e) => !changedOnly || e.git);
      entries.forEach((entry, i) => {
        const path = joinPath(dir, entry.name);
        const isDir = entry.kind === "dir";
        const expanded = isDir && this.expanded.has(path);
        const ignored = ignoredDir || entry.ignored;
        if (isDir && ignored) this.ignoredDirs.add(path);
        rows.push({
          path,
          name: entry.name,
          depth,
          kind: entry.kind,
          git: entry.git,
          renamedFrom: entry.renamed_from ?? null,
          ignored,
          expanded,
          loading: expanded && this.loading.has(path),
          posinset: i + 1,
          setsize: entries.length,
        });
        if (expanded) walk(path, depth + 1, ignored);
      });
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

/** The next row after `from` (wrapping) whose name starts with `prefix`, ignoring case; -1 if none. */
export function typeaheadMatch(rows: Row[], from: number, prefix: string): number {
  const p = prefix.toLowerCase();
  for (let step = 0; step < rows.length; step++) {
    const i = (from + step) % rows.length;
    if (rows[i]?.name.toLowerCase().startsWith(p)) return i;
  }
  return -1;
}
