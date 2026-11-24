<script lang="ts">
// Virtualized file tree: only the rows in view (plus a few) are in the DOM,
// so a project with 100k files scrolls like one with ten. Directories load
// lazily on expand; `fs.changed` / `fs.git_changed` events reload what changed.
import type { GitStatus, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { parentOf, type Row, TreeModel, visibleRange } from "./model";

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
let selected = $state<string | null>(null);
let scrollTop = $state(0);
let height = $state(0);
let viewport: HTMLDivElement | undefined = $state();

// svelte-ignore state_referenced_locally
const model = new TreeModel(
  async (path) => (await transport.call({ method: "fs.list_dir", params: { project, path } })).data.entries,
  () => {
    rows = model.rows;
    error = model.error;
    rootLoading = model.isLoading("");
  },
);

$effect(() => {
  void model.start();
  return transport.onEvent((event) => {
    if (event.type === "fs.changed" && event.data.project === project) void model.refreshPaths(event.data.paths);
    if (event.type === "fs.git_changed" && event.data.project === project) void model.refreshAll();
  });
});

let range = $derived(visibleRange(scrollTop, height, ROW_HEIGHT, rows.length, OVERSCAN));
let visible = $derived(rows.slice(range.start, range.end));
let current = $derived(selected ?? openPath);

function activate(row: Row): void {
  selected = row.path;
  if (row.kind === "dir") void model.toggle(row.path);
  else onopen(row.path);
}

function select(index: number): void {
  const row = rows[index];
  if (!row) return;
  selected = row.path;
  if (!viewport) return;
  const top = index * ROW_HEIGHT;
  if (top < viewport.scrollTop) viewport.scrollTop = top;
  else if (top + ROW_HEIGHT > viewport.scrollTop + viewport.clientHeight)
    viewport.scrollTop = top + ROW_HEIGHT - viewport.clientHeight;
  scrollTop = viewport.scrollTop;
}

function onkeydown(e: KeyboardEvent): void {
  const index = rows.findIndex((r) => r.path === current);
  const row = rows[index];
  switch (e.key) {
    case "ArrowDown":
      select(index < 0 ? 0 : Math.min(rows.length - 1, index + 1));
      break;
    case "ArrowUp":
      select(Math.max(0, index - 1));
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
      return;
  }
  e.preventDefault();
}
</script>

<div
  class="viewport"
  role="tree"
  aria-label="Files"
  tabindex="0"
  data-testid="file-tree"
  bind:this={viewport}
  bind:clientHeight={height}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  {onkeydown}
>
  {#if error}
    <p class="note error" role="alert">{error}</p>
  {:else if rows.length === 0}
    <p class="note">{rootLoading ? "Loading…" : "Empty project"}</p>
  {/if}
  <div class="spacer" style:height="{rows.length * ROW_HEIGHT}px">
    {#each visible as row, i (row.path)}
      <!-- Keys are handled once on the tree, which tracks the selected row. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        class:selected={row.path === current}
        class:ignored={row.ignored}
        role="treeitem"
        tabindex="-1"
        aria-level={row.depth + 1}
        aria-expanded={row.kind === "dir" ? row.expanded : undefined}
        aria-selected={row.path === current}
        aria-posinset={range.start + i + 1}
        aria-setsize={rows.length}
        data-path={row.path}
        style:top="{(range.start + i) * ROW_HEIGHT}px"
        style:padding-left="{8 + row.depth * 14}px"
        onclick={() => activate(row)}
      >
        <span class="twisty">{row.kind === "dir" ? (row.expanded ? "▾" : "▸") : ""}</span>
        <span class="name git-{row.git ?? 'clean'}">{row.name}</span>
        {#if row.loading}<span class="hint">…</span>{/if}
        {#if row.git}
          <span class="badge git-{row.git}" title={row.git}>{row.kind === "dir" ? "•" : GIT_LETTER[row.git]}</span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .viewport {
    position: relative;
    height: 100%;
    overflow: auto;
    outline: none;
    contain: strict;
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
  .ignored {
    opacity: 0.5;
  }
  .twisty {
    width: 12px;
    flex: none;
    color: #8a8a96;
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
  .hint,
  .note {
    color: #6f6f7a;
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
  .git-deleted,
  .git-conflicted {
    color: #f14c4c;
  }
  .git-renamed {
    color: #4fc1ff;
  }
</style>
