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
  /** An expanded directory whose listing could not be loaded; retried on expand or the next refresh. */
  failed: boolean;
  /** Position among its siblings (from 1) and their count, for `aria-posinset` / `aria-setsize`. */
  posinset: number;
  setsize: number;
  /** The row where a new file or folder gets its name (see `TreeModel.setDraft`). */
  draft?: true;
};

export type Filter = "all" | "changed";

/** Loads the direct children of a directory (`""` is the root). Rejects with a CoreError. */
export type LoadDir = (path: string) => Promise<DirEntry[]>;

export function joinPath(dir: string, name: string): string {
  return dir === "" ? name : `${dir}/${name}`;
}

/** The path of the draft row in `dir`; no real path ends with a slash. */
export function draftPath(dir: string): string {
  return `${dir}/`;
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
  /**
   * Folders whose last load failed (for example with the core disconnected).
   * They are skipped by refresh passes until the next invalidate, so a
   * failing load cannot be retried in a tight loop.
   */
  private readonly failed = new Set<string>();
  /** Restored folders not loaded yet; dropped if their parent's listing no longer has them. */
  private readonly restoring = new Set<string>();
  /** Loaded directories whose listing may be out of date; reloaded when shown. */
  private readonly stale = new Set<string>();
  private readonly ignoredDirs = new Set<string>();
  /** Paths of the rows on screen, and the folders those rows sit in; null means everything. */
  private view: { rows: Set<string>; dirs: Set<string> } | null = null;
  private pumping: Promise<void> | null = null;
  /** Expanded directories before the "changed" filter opened more. */
  private expandedBeforeFilter: Set<string> | null = null;
  /** A new file or folder being named, shown as the first row of its folder. */
  private draft: { dir: string; kind: EntryKind } | null = null;
  /** Paths the person trashed, renamed or moved away; a deleted folder git still lists there is hidden. */
  private readonly gone = new Set<string>();

  constructor(
    private readonly load: LoadDir,
    private readonly onChange: () => void,
  ) {}

  /**
   * Loads the root listing and restores `expanded` (saved from the last
   * session). Restored folders load only once their row is on screen, so a
   * tree saved with hundreds of open folders costs one screenful of requests.
   */
  async start(expanded: string[] = []): Promise<void> {
    for (const dir of expanded) {
      if (dir === "") continue;
      this.expanded.add(dir);
      this.restoring.add(dir);
    }
    await this.fetchFirst("");
    await this.pump();
  }

  /** Expanded directories, outermost first, for saving the tree's shape. */
  expandedDirs(): string[] {
    return [...(this.expandedBeforeFilter ?? this.expanded)].filter((d) => d !== "").sort();
  }

  isLoading(path: string): boolean {
    return this.loading.has(path);
  }

  /** The loaded listing of `dir`, or undefined if it is not loaded. */
  entries(dir: string): DirEntry[] | undefined {
    return this.dirs.get(dir);
  }

  /** Shows (or with null removes) a draft row as the first child of `dir`, which should be expanded. */
  setDraft(draft: { dir: string; kind: EntryKind } | null): void {
    this.draft = draft;
    this.rebuild();
  }

  /** Reloads a loaded listing now, shown or not (after the person changed it). */
  async reload(dir: string): Promise<void> {
    if (!this.dirs.has(dir)) return;
    this.stale.delete(dir);
    await this.fetch(dir);
    this.rebuild();
  }

  isExpanded(path: string): boolean {
    return this.expanded.has(path);
  }

  async expand(path: string): Promise<void> {
    // Restored folders are expanded but load lazily; expanding one loads it now (Reveal needs that).
    if (this.expanded.has(path) && (this.dirs.has(path) || this.loading.has(path))) return;
    this.expanded.add(path);
    this.failed.delete(path); // expanding again is an explicit retry
    this.rebuild();
    if (!this.dirs.has(path)) await this.fetchFirst(path);
    else await this.pump();
  }

  collapse(path: string): void {
    if (path === "" || !this.expanded.delete(path)) return;
    this.rebuild();
  }

  /** `from` was renamed or moved to `to`: folders open in it stay open at the new path. */
  moved(from: string, to: string): void {
    if (from === "") return;
    this.gone.add(from);
    for (const key of [...this.expanded]) {
      if (!isInside(key, from)) continue;
      this.expanded.delete(key);
      this.expanded.add(to + key.slice(from.length));
    }
  }

  /** The person trashed `paths`: git may still list them as deleted, but the tree does not. */
  removed(paths: string[]): void {
    for (const path of paths) this.gone.add(path);
  }

  /**
   * Whether a listed entry is left out of the full tree. As in other
   * editors, a tracked file deleted from disk shows only under "changed"
   * (and in Git); so does a deleted folder the person removed or whose
   * loaded listing holds nothing else. A folder that exists and only holds
   * a deleted file carries the same status, so an unloaded one stays.
   */
  private hidden(path: string, entry: DirEntry): boolean {
    if (entry.git !== "deleted" || this.filter === "changed") return false;
    if (entry.kind !== "dir" || this.gone.has(path)) return true;
    return this.dirs.get(path)?.every((e) => this.hidden(joinPath(path, e.name), e)) ?? false;
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

  /** The paths of the rows on screen; listings and restored folders outside it load when they scroll in. */
  setView(rows: Set<string> | null): Promise<void> {
    this.view = rows && { rows, dirs: new Set([...rows].map(parentOf)) };
    return this.pump();
  }

  /**
   * An `fs.changed` event: the listings of the changed paths' folders, and of
   * changed folders with everything loaded under them (the watcher reports
   * big batches as their top-level folders), are out of date.
   */
  invalidatePaths(paths: string[]): Promise<void> {
    for (const path of paths) {
      for (const dir of [...this.failed]) if (isInside(dir, path) || dir === parentOf(path)) this.failed.delete(dir);
      if (this.dirs.has(parentOf(path))) this.stale.add(parentOf(path));
      for (const dir of this.dirs.keys()) if (isInside(dir, path)) this.stale.add(dir);
    }
    return this.pump();
  }

  /** An `fs.git_changed` event: every loaded listing carries git status, so all may be out of date. */
  invalidateAll(): Promise<void> {
    this.failed.clear();
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
    // A pass in flight may already have done its last check; run again after it.
    if (this.pumping) return this.pumping.then(() => this.pump());
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

  /** Stale listings with rows on screen, and expanded folders not loaded yet whose own row is on screen. */
  private due(): string[] {
    const view = this.view;
    const stale = [...this.stale].filter((d) => this.isShown(d) && (!view || view.dirs.has(d)));
    const unloaded = [...this.expanded].filter(
      (d) =>
        !this.dirs.has(d) &&
        !this.loading.has(d) &&
        !this.failed.has(d) &&
        this.listed(d) &&
        this.isShown(d) &&
        (!view || view.rows.has(d)),
    );
    return [...stale, ...unloaded];
  }

  /** Saved folders under `dir` that no longer exist are dropped with everything saved under them. */
  private dropMissingRestored(dir: string, entries: DirEntry[]): void {
    this.restoring.delete(dir);
    const names = new Set(entries.map((e) => e.name));
    for (const path of [...this.restoring]) {
      if (parentOf(path) !== dir || names.has(path.slice(path.lastIndexOf("/") + 1))) continue;
      for (const key of [...this.restoring]) if (isInside(key, path)) this.restoring.delete(key);
      for (const key of [...this.expanded]) if (isInside(key, path)) this.expanded.delete(key);
    }
  }

  /** Whether `path` is an entry of its parent's loaded listing. */
  private listed(path: string): boolean {
    const name = path.slice(path.lastIndexOf("/") + 1);
    return this.dirs.get(parentOf(path))?.some((e) => e.name === name) ?? false;
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
    const wasListed = this.dirs.has(dir);
    try {
      const entries = sortEntries(await this.load(dir));
      this.forgetMissingChildren(dir, entries);
      this.dirs.set(dir, entries);
      this.failed.delete(dir);
      this.dropMissingRestored(dir, entries);
      this.error = null;
    } catch (err) {
      this.failed.add(dir);
      // A CoreError carries the core's message without its `code:` prefix.
      const core = err as { code?: string; error?: { message: string } };
      // A folder that was listed before keeps its expanded state, in case it comes back.
      if (core.code === "not_found" && dir !== "") this.forget(dir, wasListed);
      else this.error = core.error?.message ?? (err instanceof Error ? err.message : String(err));
    }
  }

  /** Drops state for child directories that are gone from a fresh listing of `dir`. */
  private forgetMissingChildren(dir: string, entries: DirEntry[]): void {
    const old = this.dirs.get(dir);
    if (!old) return;
    const present = new Set(entries.filter((e) => e.kind === "dir").map((e) => e.name));
    for (const entry of old) {
      if (entry.kind === "dir" && !present.has(entry.name)) this.forget(joinPath(dir, entry.name), true);
    }
  }

  /**
   * Drops what is known about `dir` and everything under it. A folder that
   * vanished from a live listing keeps its expanded state (`keepExpanded`), so
   * one that a build deletes and writes again comes back open.
   */
  private forget(dir: string, keepExpanded = false): void {
    for (const key of [...this.dirs.keys()]) if (isInside(key, dir)) this.dirs.delete(key);
    if (!keepExpanded) for (const key of [...this.expanded]) if (isInside(key, dir)) this.expanded.delete(key);
    for (const key of [...this.stale]) if (isInside(key, dir)) this.stale.delete(key);
    for (const key of [...this.failed]) if (isInside(key, dir)) this.failed.delete(key);
  }

  private rebuild(): void {
    const rows: Row[] = [];
    const changedOnly = this.filter === "changed";
    this.ignoredDirs.clear();
    const walk = (dir: string, depth: number, ignoredDir: boolean): void => {
      const entries = (this.dirs.get(dir) ?? []).filter(
        (e) => (!changedOnly || e.git) && !this.hidden(joinPath(dir, e.name), e),
      );
      const draft = this.draft?.dir === dir ? this.draft : null;
      const setsize = entries.length + (draft ? 1 : 0);
      if (draft) {
        rows.push({
          path: draftPath(dir),
          name: "",
          depth,
          kind: draft.kind,
          git: null,
          renamedFrom: null,
          ignored: false,
          expanded: false,
          loading: false,
          failed: false,
          posinset: 1,
          setsize,
          draft: true,
        });
      }
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
          loading: expanded && !this.dirs.has(path) && !this.failed.has(path),
          failed: expanded && this.failed.has(path) && !this.dirs.has(path),
          posinset: i + 1 + (draft ? 1 : 0),
          setsize,
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
