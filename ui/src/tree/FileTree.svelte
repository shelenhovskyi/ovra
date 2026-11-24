<script lang="ts">
// Virtualized file tree: only the rows in view (plus a few) are in the DOM,
// so a project with 100k files scrolls like one with ten. Focus stays on the
// tree and `aria-activedescendant` names the active row, which is always
// rendered. Folders load lazily; fs events mark listings stale and only the
// ones on screen reload.
import { registerCommand } from "../keys/commands";
import type { GitStatus, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { ancestorsOf, type Filter, parentOf, type Row, TreeModel, typeaheadMatch, visibleRange } from "./model";
import { ExpandedSaver, loadExpanded } from "./persist";

let {
  transport,
  project,
  openPath = null,
  onopen,
}: {
  transport: Transport;
  project: ProjectId;
  openPath?: string | null;
  onopen: (path: string) => void;
} = $props();

const ROW_HEIGHT = 22;
const OVERSCAN = 8;
const TYPEAHEAD_RESET_MS = 700;
const ID = `tree-${Math.random().toString(36).slice(2, 8)}`;

const GIT_LETTER: Record<GitStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  untracked: "U",
  conflicted: "C",
};

let rows = $state.raw<Row[]>([]);
let error = $state<string | null>(null);
let rootLoading = $state(true);
let filter = $state<Filter>("all");
/** Path of the active row; survives rebuilds. */
let selected = $state<string | null>(null);
let scrollTop = $state(0);
let height = $state(0);
let viewport: HTMLDivElement | undefined = $state();
let typed = { text: "", at: 0 };

// svelte-ignore state_referenced_locally
const model = new TreeModel(
  async (path) => (await transport.call({ method: "fs.list_dir", params: { project, path } })).data.entries,
  () => update(model.rows),
);
let saver: ExpandedSaver | null = null;

/** Takes new rows, keeping the selection and the rows under the user's eyes in place. */
function update(next: Row[]): void {
  const prev = rows;
  const anchorIndex = Math.floor(scrollTop / ROW_HEIGHT);
  const anchor = scrollTop > 0 ? prev[anchorIndex]?.path : undefined;
  if (selected !== null && !next.some((r) => r.path === selected)) selected = fallback(prev, next, selected);
  rows = next;
  error = model.error;
  rootLoading = model.isLoading("");
  filter = model.filter;
  saver?.update(model.expandedDirs());
  if (anchor && viewport) {
    const moved = next.findIndex((r) => r.path === anchor);
    if (moved >= 0 && moved !== anchorIndex) {
      viewport.scrollTop += (moved - anchorIndex) * ROW_HEIGHT;
      scrollTop = viewport.scrollTop;
    }
  }
}

/** Where the selection goes when its row disappears: the collapsed folder hiding it, else the row now at its place. */
function fallback(prev: Row[], next: Row[], path: string): string | null {
  for (const dir of ancestorsOf(path).reverse()) {
    const row = next.find((r) => r.path === dir);
    if (row) {
      if (!row.expanded) return dir;
      break;
    }
  }
  const at = prev.findIndex((r) => r.path === path);
  if (at < 0 || next.length === 0) return null;
  return next[Math.min(at, next.length - 1)]?.path ?? null;
}

$effect(() => {
  void loadExpanded(transport, project).then((expanded) => {
    // Save only once restored, so the first rebuilds cannot overwrite the saved shape.
    void model.start(expanded).then(() => {
      started = true;
      saver = new ExpandedSaver(transport, project, expanded);
      saver.update(model.expandedDirs());
    });
  });
  viewport?.focus();
  const off = transport.onEvent((event) => {
    if (event.type === "fs.changed" && event.data.project === project) void model.invalidatePaths(event.data.paths);
    if (event.type === "fs.git_changed" && event.data.project === project) void model.invalidateAll();
    // Events may have been dropped while this client lagged behind.
    if (event.type === "info.resync") void model.invalidateAll();
  });
  // The core does not watch ignored folders; re-list the shown ones when the window comes back.
  const onfocus = () => void model.invalidateIgnored();
  // Closing or reloading the window does not unmount the tree; save then too.
  // The save is one message sent synchronously, so it leaves before the socket closes.
  const flush = () => void saver?.flush();
  window.addEventListener("focus", onfocus);
  window.addEventListener("pagehide", flush);
  const commands = [
    registerCommand({ id: "tree.collapse_all", title: "Collapse all folders", run: collapseAll }),
    registerCommand({ id: "tree.reveal", title: "Reveal open file in the tree", run: reveal }),
    registerCommand({ id: "tree.changed_only", title: "Show changed files only (toggle)", run: toggleChanged }),
  ];
  return () => {
    off();
    for (const unregister of commands) unregister();
    window.removeEventListener("focus", onfocus);
    window.removeEventListener("pagehide", flush);
    flush();
  };
});

let range = $derived(visibleRange(scrollTop, height, ROW_HEIGHT, rows.length, OVERSCAN));
let current = $derived(selected ?? openPath);
let activeIndex = $derived(current === null ? -1 : rows.findIndex((r) => r.path === current));
/** Rendered rows: the window, plus the active row wherever it is, so `aria-activedescendant` always resolves. */
let shown = $derived.by(() => {
  const indexes = [];
  for (let i = range.start; i < range.end; i++) indexes.push(i);
  if (activeIndex >= 0 && (activeIndex < range.start || activeIndex >= range.end)) indexes.push(activeIndex);
  return indexes;
});

// Only listings with rows on screen reload after fs events, and restored folders load
// when their row comes on screen; the rest wait until scrolled to.
$effect(() => {
  void model.setView(new Set(rows.slice(range.start, range.end).map((r) => r.path)));
});

// Show the open file in the tree (after a restore, a tab switch or the palette), without taking focus.
let started = $state(false);
$effect(() => {
  const path = openPath;
  if (!started || !path) return;
  // While filtered to changed files, do not undo the filter just to show a file.
  const shown = model.filter === "all" ? model.reveal(path) : Promise.resolve();
  void shown.then(() => {
    if (openPath === path) select(rows.findIndex((r) => r.path === path));
  });
});

function rowId(index: number): string {
  return `${ID}-${index}`;
}

function describe(row: Row): string {
  const parts = [];
  if (row.git) parts.push(row.kind === "dir" ? `contains ${row.git} files` : row.git);
  if (row.renamedFrom) parts.push(`from ${row.renamedFrom}`);
  if (row.ignored) parts.push("ignored");
  if (row.loading) parts.push("loading");
  return parts.join(", ");
}

function activate(row: Row): void {
  selected = row.path;
  viewport?.focus(); // a click focuses the row itself; focus belongs on the tree
  if (row.kind === "dir") void model.toggle(row.path);
  // A deleted file is listed so the change is visible, but there is nothing on disk to open.
  else if (row.git !== "deleted") onopen(row.path);
}

function select(index: number, center = false): void {
  const row = rows[index];
  if (!row) return;
  selected = row.path;
  if (!viewport) return;
  const top = index * ROW_HEIGHT;
  const view = viewport.clientHeight;
  if (center) viewport.scrollTop = Math.max(0, top - view / 2);
  else if (top < viewport.scrollTop) viewport.scrollTop = top;
  else if (top + ROW_HEIGHT > viewport.scrollTop + view) viewport.scrollTop = top + ROW_HEIGHT - view;
  scrollTop = viewport.scrollTop;
}

function collapseAll(): void {
  model.collapseAll();
  viewport?.focus();
}

async function reveal(): Promise<void> {
  if (!openPath) return;
  const path = openPath;
  await model.reveal(path);
  select(
    rows.findIndex((r) => r.path === path),
    true,
  );
  viewport?.focus();
}

function toggleChanged(): void {
  void model.setFilter(model.filter === "changed" ? "all" : "changed");
}

function onfocus(): void {
  if (current === null && rows.length > 0) selected = rows[0]?.path ?? null;
}

function typeahead(e: KeyboardEvent): boolean {
  if (e.key.length !== 1 || e.ctrlKey || e.metaKey || e.altKey) return false;
  const now = Date.now();
  const fresh = now - typed.at > TYPEAHEAD_RESET_MS;
  if (fresh && e.key === " ") return false;
  typed = { text: fresh ? e.key : typed.text + e.key, at: now };
  // A new search starts after the active row; a longer prefix may still match it.
  const from = Math.max(0, activeIndex) + (fresh ? 1 : 0);
  const match = typeaheadMatch(rows, from % Math.max(1, rows.length), typed.text);
  if (match >= 0) select(match);
  return true;
}

function onkeydown(e: KeyboardEvent): void {
  const index = activeIndex;
  const row = rows[index];
  const page = Math.max(1, Math.floor(height / ROW_HEIGHT) - 1);
  switch (e.key) {
    case "ArrowDown":
      select(index < 0 ? 0 : Math.min(rows.length - 1, index + 1));
      break;
    case "ArrowUp":
      select(Math.max(0, index - 1));
      break;
    case "PageDown":
      select(Math.min(rows.length - 1, Math.max(0, index) + page));
      break;
    case "PageUp":
      select(Math.max(0, index - page));
      break;
    case "Home":
      select(0);
      break;
    case "End":
      select(rows.length - 1);
      break;
    case "ArrowRight":
      if (row?.kind !== "dir") return;
      if (row.expanded) select(index + 1);
      else void model.expand(row.path);
      break;
    case "ArrowLeft":
      if (row?.kind === "dir" && row.expanded) model.collapse(row.path);
      else if (row) select(rows.findIndex((r) => r.path === parentOf(row.path)));
      break;
    case "Enter":
      if (row) activate(row);
      break;
    default:
      if (!typeahead(e)) return;
  }
  e.preventDefault();
}
</script>

<div class="panel">
  <div class="toolbar">
    <button type="button" onclick={collapseAll} title="Collapse all folders">Collapse all</button>
    <button type="button" onclick={reveal} disabled={!openPath} title="Show the open file in the tree">Reveal</button>
    <button type="button" onclick={toggleChanged} aria-pressed={filter === "changed"} title="Show only changed files">
      Changed only
    </button>
  </div>
  <div
    class="viewport"
    role="tree"
    aria-label="Files"
    aria-activedescendant={activeIndex >= 0 ? rowId(activeIndex) : undefined}
    tabindex="0"
    data-testid="file-tree"
    bind:this={viewport}
    bind:clientHeight={height}
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    {onkeydown}
    {onfocus}
  >
    {#if error}
      <p class="note error" role="alert">{error}</p>
    {:else if rows.length === 0}
      <p class="note">
        {rootLoading ? "Loading…" : filter === "changed" ? "No changed files" : "Empty project"}
      </p>
    {/if}
    <div class="spacer" style:height="{rows.length * ROW_HEIGHT}px">
      {#each shown as index (rows[index]?.path ?? index)}
        {@const row = rows[index]}
        {#if row}
          <!-- Keys are handled once on the tree, which tracks the active row. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id={rowId(index)}
            class="row"
            class:selected={index === activeIndex}
            class:ignored={row.ignored}
            role="treeitem"
            aria-label={row.name}
            tabindex="-1"
            aria-describedby={describe(row) ? `${rowId(index)}-d` : undefined}
            aria-level={row.depth + 1}
            aria-expanded={row.kind === "dir" ? row.expanded : undefined}
            aria-selected={index === activeIndex}
            aria-posinset={row.posinset}
            aria-setsize={row.setsize}
            data-path={row.path}
            data-git={row.git ?? undefined}
            data-ignored={row.ignored || undefined}
            title={describe(row) ? `${row.path} (${describe(row)})` : row.path}
            style:top="{index * ROW_HEIGHT}px"
            style:padding-left="{8 + row.depth * 14}px"
            onclick={() => activate(row)}
          >
            <span class="twisty" aria-hidden="true">{row.kind === "dir" ? (row.expanded ? "▾" : "▸") : ""}</span>
            <span class="name git-{row.git ?? 'clean'}">{row.name}</span>
            {#if row.loading}<span class="hint" aria-hidden="true">…</span>{/if}
            {#if describe(row)}<span id="{rowId(index)}-d" class="visually-hidden">{describe(row)}</span>{/if}
            {#if row.git}
              <span class="badge git-{row.git}" aria-hidden="true">{row.kind === "dir" ? "•" : GIT_LETTER[row.git]}</span>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .toolbar {
    display: flex;
    gap: 4px;
    padding: 4px 6px;
    border-bottom: 1px solid #2a2a31;
  }
  .toolbar button {
    padding: 2px 6px;
    border: 1px solid #2a2a31;
    border-radius: 3px;
    background: #1b1b20;
    color: #c8c8d0;
    font: inherit;
    font-size: 12px;
  }
  .toolbar button[aria-pressed="true"] {
    background: #2a2d3a;
    border-color: #4c6ef5;
  }
  .toolbar button:disabled {
    color: #9a9aa6;
  }
  .viewport {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
    contain: strict;
  }
  .viewport:focus-visible {
    box-shadow: inset 0 0 0 1px #4c6ef5;
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 22px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 8px;
    white-space: nowrap;
    cursor: default;
    user-select: none;
  }
  .row:hover {
    background: #1f1f25;
  }
  .row.selected {
    background: #2a2d3a;
  }
  .viewport:focus-visible .row.selected {
    outline: 1px solid #4c6ef5;
    outline-offset: -1px;
  }
  .ignored .name {
    color: #9a9aa6;
    font-style: italic;
  }
  .twisty {
    width: 12px;
    flex: none;
    color: #9a9aa6;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badge {
    flex: none;
    font-size: 11px;
    font-weight: 600;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .hint,
  .note {
    color: #9a9aa6;
  }
  .note {
    margin: 0;
    padding: 12px;
  }
  .error {
    color: #ff6b6b;
  }
  .git-modified {
    color: #e2c08d;
  }
  .git-added,
  .git-untracked {
    color: #73c991;
  }
  .name.git-deleted {
    text-decoration: line-through;
  }
  .git-deleted,
  .git-conflicted {
    color: #f14c4c;
  }
  .git-renamed {
    color: #4fc1ff;
  }
</style>
