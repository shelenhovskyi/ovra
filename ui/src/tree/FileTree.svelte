<script lang="ts" module>
import { registerContextKeys } from "../keys/context-keys";
import { trashKeys } from "./ops";

// This panel's own keys, for Settings > Keyboard; the panel handles them itself.
registerContextKeys(
  [
    { keys: "Enter", title: "Open the file, or keep its tab when it is shown" },
    { keys: "F2", title: "Rename" },
    { keys: trashKeys(), title: "Move to the trash" },
    { keys: "Shift+Up/Down", title: "Select a range of rows (Mod+click adds one)" },
    { keys: "Mod+X", title: "Cut" },
    { keys: "Mod+C", title: "Copy, also as paths for other apps" },
    { keys: "Mod+V", title: "Paste into the folder" },
    { keys: "Escape", title: "Clear the selection and the cut" },
    { keys: "Mod+Z", title: "Undo the last file operation" },
    { keys: "Mod+Shift+Z", title: "Redo the file operation" },
    { keys: "Shift+F10", title: "Open the row's menu (also the Menu key)" },
  ].map((key) => ({ ...key, where: "Files" })),
);
</script>

<script lang="ts">
// Virtualized file tree: only the rows in view (plus a few) are in the DOM,
// so a project with 100k files scrolls like one with ten. Focus stays on the
// tree and `aria-activedescendant` names the active row, which is always
// rendered. Folders load lazily; fs events mark listings stale and only the
// ones on screen reload.
import { listCommands, registerCommand } from "../keys/commands";
import type { GitStatus, ProjectId } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { isMenuKey, openContextMenu } from "../menu/context-menu.svelte";
import EmptyState from "../shell/EmptyState.svelte";
import Icon from "../shell/Icon.svelte";
import type { MainMode } from "../shell/workspace.svelte";
import { FileActions } from "./actions.svelte";
import { folderFor, registerFileCommands } from "./commands";
import { HoverExpander } from "./hover-expand";
import { rootMenu, rowMenu, selectionMenu } from "./menu";
import { ancestorsOf, type Filter, parentOf, type Row, TreeModel, typeaheadMatch, visibleRange } from "./model";
import NameInput from "./NameInput.svelte";
import { keyAction, moveTarget, stemRange } from "./ops";
import { ExpandedSaver, loadExpanded } from "./persist";
import { clickKind, rangeOf, toggled, topLevel } from "./selection";
import TrashConfirm from "./TrashConfirm.svelte";
import WatchLimitNotice from "./WatchLimitNotice.svelte";

let {
  transport,
  project,
  root = "",
  openPath = null,
  onopen,
  onkeep,
  onreveal,
  onmoved,
  ontrashed,
}: {
  transport: Transport;
  project: ProjectId;
  /** The project folder on disk, for Copy Path. */
  root?: string;
  openPath?: string | null;
  /** Opens a file; a file the person just made opens in `"edit"`. */
  onopen: (path: string, mode?: MainMode) => void;
  /** A double click on a file: keep its tab instead of letting the next file replace it. */
  onkeep?: (path: string) => void;
  /** Before a reveal: make the tree visible (the sidebar may show another view). */
  onreveal?: () => void;
  /** A file or folder was renamed or moved by the person. */
  onmoved?: (from: string, to: string) => void;
  ontrashed?: (paths: string[]) => void;
} = $props();

const ROW_HEIGHT = 28;
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
/** Folders the core polls instead of watching (`fs.watch_limited`). */
let unwatched = $state(0);
/** Path of the active row; survives rebuilds. */
let selected = $state<string | null>(null);
/** Rows chosen with Mod or Shift, in the order chosen; empty when only the active row is selected. */
let picked = $state.raw<string[]>([]);
/** Where a Shift range starts. */
let anchor: string | null = null;
let scrollTop = $state(0);
let height = $state(0);
let viewport: HTMLDivElement | undefined = $state();
let typed = { text: "", at: 0 };
/** A selected path whose row went away, and the row the selection moved to; restored if it comes back. */
let lost: { path: string; movedTo: string | null } | null = null;

// svelte-ignore state_referenced_locally
const model = new TreeModel(
  async (path) => (await transport.call({ method: "fs.list_dir", params: { project, path } })).data.entries,
  () => update(model.rows),
);
let saver: ExpandedSaver | null = null;

// svelte-ignore state_referenced_locally
const actions = new FileActions({
  transport,
  project,
  model,
  root: () => root,
  selected: () => current,
  select: selectPath,
  open: (path) => onopen(path, "edit"),
  moved: (from, to) => onmoved?.(from, to),
  trashed: (paths) => ontrashed?.(paths),
});

/** Takes new rows, keeping the selection and the rows under the user's eyes in place. */
function update(next: Row[]): void {
  const prev = rows;
  const anchorIndex = Math.floor(scrollTop / ROW_HEIGHT);
  const anchor = scrollTop > 0 ? prev[anchorIndex]?.path : undefined;
  if (lost && selected === lost.movedTo && next.some((r) => r.path === lost?.path)) {
    selected = lost.path; // a regenerated file or folder came back: select it again
    lost = null;
  } else if (selected !== null && !next.some((r) => r.path === selected)) {
    const movedTo = fallback(prev, next, selected);
    lost = { path: selected, movedTo };
    selected = movedTo;
  }
  if (picked.length > 0) {
    const present = new Set(next.map((r) => r.path));
    picked = picked.filter((p) => present.has(p));
  }
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
    if (event.type === "fs.watch_limited" && event.data.project === project) unwatched = event.data.unwatched;
    // Events may have been dropped while this client lagged behind.
    if (event.type === "info.resync") void model.invalidateAll();
  });
  // After a reconnect nothing tells us what changed during the outage: re-read what is shown.
  let wasClosed = false;
  const offStatus = transport.onStatus?.((status) => {
    if (status === "closed") wasClosed = true;
    else if (status === "open" && wasClosed) {
      wasClosed = false;
      void model.invalidateAll();
    }
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
    registerCommand({ id: "tree.next_change", title: "Go to next changed file in the tree", run: () => nextChange(1) }),
    registerCommand({
      id: "tree.previous_change",
      title: "Go to previous changed file in the tree",
      run: () => nextChange(-1),
    }),
    registerFileCommands(actions, activeRow, () => onreveal?.()),
  ];
  return () => {
    off();
    offStatus?.();
    for (const unregister of commands) unregister();
    window.removeEventListener("focus", onfocus);
    window.removeEventListener("pagehide", flush);
    flush();
  };
});

let range = $derived(visibleRange(scrollTop, height, ROW_HEIGHT, rows.length, OVERSCAN));
let current = $derived(selected ?? openPath);
let activeIndex = $derived(current === null ? -1 : rows.findIndex((r) => r.path === current));
/** The selected paths: the picked rows, or else the active one. */
let chosen = $derived(picked.length > 0 ? picked : current !== null && activeIndex >= 0 ? [current] : []);
let chosenSet = $derived(new Set(chosen));
/** Rows cut and not pasted yet: they look dimmed, and so does what is in a cut folder. */
let cutPaths = $derived(actions.clip?.cut ? actions.clip.paths : []);
const isCut = (path: string) => cutPaths.some((p) => path === p || path.startsWith(`${p}/`));
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
  if (row.failed) parts.push("could not load, collapse and expand to retry");
  return parts.join(", ");
}

/** The row's path and description; a file's git letter leads, so the dot's colour is not the only sign. */
function tooltip(row: Row): string {
  const description = describe(row);
  if (!description) return row.path;
  const letter = row.git && row.kind !== "dir" ? `${GIT_LETTER[row.git]}: ` : "";
  return `${row.path} (${letter}${description})`;
}

/** The paths being dragged inside the tree, and the folder they would drop into (`""` is the project folder). */
let dragging: string[] | null = null;
let dropDir = $state<string | null>(null);
const DRAG_TYPE = "application/x-ovra-path";
const hoverExpand = new HoverExpander((path) => void model.expand(path));
$effect(() => () => hoverExpand.cancel());

/** The folder a drop on `target` moves into: the row's folder, or the project folder below the rows. */
function dropDirAt(target: EventTarget | null): string | null {
  const el = target instanceof Element ? target.closest<HTMLElement>("[data-path]") : null;
  const path = el?.dataset.path;
  if (path?.endsWith("/")) return null; // the draft row
  const dir = !el || !path ? "" : el.hasAttribute("aria-expanded") ? path : parentOf(path);
  if (dragging === null) return null;
  // Not into one of the dragged folders, and somewhere at least one of them is not already.
  if (dragging.some((p) => dir === p || dir.startsWith(`${p}/`))) return null;
  return dragging.some((p) => moveTarget(p, dir) !== null) ? dir : null;
}

function ondragstart(e: DragEvent): void {
  const path = e.target instanceof Element ? e.target.closest<HTMLElement>("[data-path]")?.dataset.path : undefined;
  if (!path) return;
  dragging = picked.length > 1 && chosenSet.has(path) ? topLevel(picked) : [path];
  e.dataTransfer?.setData(DRAG_TYPE, dragging.join("\n"));
  if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
}

// Drops from outside the tree (files from the system) are not taken yet.
function ondragover(e: DragEvent): void {
  if (dragging === null) return;
  dropDir = dropDirAt(e.target);
  const el = e.target instanceof Element ? e.target.closest<HTMLElement>("[data-path]") : null;
  const collapsed = el?.getAttribute("aria-expanded") === "false" && el.dataset.path === dropDir;
  hoverExpand.over(collapsed ? dropDir : null);
  if (dropDir === null) return;
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
}

function ondrop(e: DragEvent): void {
  const from = dragging;
  if (from === null) return;
  e.preventDefault();
  const dir = dropDirAt(e.target);
  ondragend();
  if (dir !== null) void actions.move(from, dir);
}

/** The drag ended, dropped or not (Escape cancels it). */
function ondragend(): void {
  dragging = null;
  dropDir = null;
  hoverExpand.cancel();
}

function ondragleave(e: DragEvent): void {
  if (e.relatedTarget instanceof Node && viewport?.contains(e.relatedTarget)) return;
  dropDir = null;
  hoverExpand.cancel();
}

/** A right-click on a row (which it selects), below the rows or on the header. */
function oncontextmenu(e: MouseEvent): void {
  if (e.target instanceof HTMLInputElement) return; // the name field keeps its own menu
  // The Menu key or Shift+F10 fires this too, on the focused tree: the active row's menu.
  if (e.button !== 2 && e.target === viewport) {
    e.preventDefault();
    menuForActive();
    return;
  }
  const el = e.target instanceof Element ? e.target.closest<HTMLElement>("[data-path]") : null;
  const path = el?.dataset.path;
  e.preventDefault();
  if (path?.endsWith("/")) return; // the draft row
  const at = { x: e.clientX, y: e.clientY, returnFocus: viewport ?? null };
  if (!el || !path) {
    openContextMenu(at, rootMenu(actions, collapseAll));
    return;
  }
  if (picked.length > 1 && chosenSet.has(path)) {
    openContextMenu(at, selectionMenu(actions, picked));
    return;
  }
  selectPath(path);
  openContextMenu(at, rowMenu(actions, { path, kind: el.hasAttribute("aria-expanded") ? "dir" : "file" }));
}

/** The active row's menu (the project's without one), under the row. */
function menuForActive(): void {
  const row = activeRow();
  const box = (row ? document.getElementById(rowId(activeIndex)) : viewport)?.getBoundingClientRect();
  const at = {
    x: (box?.left ?? 0) + 24,
    y: row ? (box?.bottom ?? 0) : (box?.top ?? 0) + 8,
    returnFocus: viewport ?? null,
  };
  if (picked.length > 1) openContextMenu(at, selectionMenu(actions, picked));
  else openContextMenu(at, row ? rowMenu(actions, row) : rootMenu(actions, collapseAll));
}

/** The active row, unless it is the draft of a new entry. */
function activeRow(): Row | null {
  const row = rows[activeIndex];
  return row && !row.draft ? row : null;
}

/** Selects the row at `path` (or nothing) and gives focus back to the tree. */
function selectPath(path: string | null): void {
  const index = path === null ? -1 : rows.findIndex((r) => r.path === path);
  if (index >= 0) select(index);
  else selected = path;
  picked = [];
  lost = null;
  viewport?.focus();
}

function activate(row: Row): void {
  if (row.draft) return;
  selected = row.path;
  picked = [];
  anchor = row.path;
  lost = null;
  viewport?.focus(); // a click focuses the row itself; focus belongs on the tree
  if (row.kind === "dir") void model.toggle(row.path);
  // A deleted file is listed so the change is visible, but there is nothing on disk to open.
  else if (row.git !== "deleted") onopen(row.path);
}

/** A click with Mod toggles the row, with Shift takes the range to it; a plain one opens it. */
function onrowclick(e: MouseEvent, row: Row): void {
  if (row.draft) return;
  const kind = clickKind(e);
  if (kind === "single") return activate(row);
  if (kind === "toggle") {
    picked = toggled(chosen, row.path);
    anchor = row.path;
  } else {
    const from = anchor ?? current ?? row.path;
    picked = rangeOf(rows, from, row.path);
    anchor = from;
  }
  selected = row.path;
  lost = null;
  viewport?.focus();
}

/** Shift+Arrow: moves the active row and selects the range from the anchor to it. */
function extendTo(index: number): void {
  const row = rows[index];
  if (!row) return;
  const from = anchor ?? current ?? row.path;
  select(index);
  anchor = from;
  picked = rangeOf(rows, from, row.path);
}

function select(index: number, center = false): void {
  const row = rows[index];
  if (!row) return;
  selected = row.path;
  picked = [];
  anchor = row.path;
  lost = null;
  if (!viewport) return;
  const top = index * ROW_HEIGHT;
  const view = viewport.clientHeight;
  if (center) viewport.scrollTop = Math.max(0, top - view / 2);
  else if (top < viewport.scrollTop) viewport.scrollTop = top;
  else if (top + ROW_HEIGHT > viewport.scrollTop + view) viewport.scrollTop = top + ROW_HEIGHT - view;
  scrollTop = viewport.scrollTop;
}

/** Selects the next (or previous) changed file among the loaded rows, wrapping around. */
function nextChange(step: 1 | -1): void {
  const n = rows.length;
  for (let k = 1; k <= n; k++) {
    const i = (((activeIndex + step * k) % n) + n) % n;
    const row = rows[i];
    if (row?.git && row.kind !== "dir") {
      select(i, true);
      viewport?.focus();
      return;
    }
  }
}

function collapseAll(): void {
  model.collapseAll();
  viewport?.focus();
}

async function reveal(): Promise<void> {
  if (!openPath) return;
  const path = openPath;
  onreveal?.();
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

function newTask(): void {
  void listCommands()
    .find((c) => c.id === "agents.new_session")
    ?.run();
}

function onfocus(): void {
  if (current === null && rows.length > 0) selected = rows[0]?.path ?? null;
}

/** Keys pressed on their own while typing a prefix (a capital, a dead key, an input method). */
const TYPING_KEYS = new Set(["Shift", "CapsLock", "AltGraph", "Dead", "Process", "Compose"]);

const typesText = (e: KeyboardEvent) => e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey;

function typeahead(e: KeyboardEvent): boolean {
  if (!typesText(e)) return false;
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
  // Any key that types nothing ends the prefix (the ARIA tree pattern), so "car Enter rea" finds README.
  if (!typesText(e) && !TYPING_KEYS.has(e.key)) typed = { text: "", at: 0 };
  const index = activeIndex;
  const row = rows[index];
  const page = Math.max(1, Math.floor(height / ROW_HEIGHT) - 1);
  const action = e.target instanceof HTMLInputElement ? null : keyAction(e);
  const real = row && !row.draft ? row : null;
  // Rename works on one row only.
  if (action === "rename" && real && picked.length <= 1) actions.startRename(real.path);
  else if (action === "trash" && real) actions.askTrash(chosen);
  else if (action === "cut" && real) actions.cut(chosen);
  else if (action === "copy" && real) void actions.copy(chosen);
  else if (action === "paste") void actions.paste(folderFor(real));
  else if (action === "undo") void actions.undo();
  else if (action === "redo") void actions.redo();
  if (action) {
    e.preventDefault();
    return;
  }
  if (isMenuKey(e)) {
    e.preventDefault();
    menuForActive();
    return;
  }
  switch (e.key) {
    case "ArrowDown":
      (e.shiftKey ? extendTo : select)(index < 0 ? 0 : Math.min(rows.length - 1, index + 1));
      break;
    case "ArrowUp":
      (e.shiftKey ? extendTo : select)(Math.max(0, index - 1));
      break;
    case "Escape":
      if (!actions.dropCut() && picked.length === 0) return;
      picked = [];
      anchor = current;
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
      // Enter on the file already shown keeps its preview tab, as a double click does.
      if (row && row.kind !== "dir" && row.path === openPath) onkeep?.(row.path);
      if (row) activate(row);
      break;
    default:
      if (!typeahead(e)) return;
  }
  e.preventDefault();
}
</script>


<div class="panel">
  <div class="toolbar" {oncontextmenu} role="presentation">
    <span class="title" aria-hidden="true">Files</span>
    <button type="button" class="icon" onclick={collapseAll} aria-label="Collapse all" title="Collapse all folders">
      <Icon name="collapse-all" small />
    </button>
    <button type="button" class="icon" onclick={reveal} disabled={!openPath} aria-label="Reveal" title="Reveal the open file">
      <Icon name="reveal" small />
    </button>
    <button
      type="button"
      class="icon"
      onclick={toggleChanged}
      aria-pressed={filter === "changed"}
      aria-label="Changed only"
      title="Show changed files only"
    >
      <Icon name="filter" small />
    </button>
  </div>
  <WatchLimitNotice {unwatched} />
  {#if actions.confirming !== null}
    <TrashConfirm
      question={actions.question}
      onconfirm={() => void actions.confirmTrash()}
      oncancel={() => actions.cancelTrash()}
    />
  {/if}
  {#if actions.notice}<p class="note error" role="alert">{actions.notice}</p>{/if}
  {#if actions.info}<p class="note" role="status">{actions.info}</p>{/if}
  <!-- Outside the tree, whose children are its rows. -->
  {#if !error && !rootLoading && rows.length === 0}
    {#if filter === "changed"}
      <EmptyState
        headline="No changed files"
        actions={[{ label: "Show all files", run: () => void model.setFilter("all") }]}
      />
    {:else}
      <EmptyState
        headline="This folder is empty"
        line="Ask Claude to set up a project here, or add files."
        actions={[{ label: "Give Claude a task", run: newTask }]}
      />
    {/if}
  {/if}
  <div
    class="viewport"
    role="tree"
    aria-label="Files"
    aria-multiselectable="true"
    aria-activedescendant={activeIndex >= 0 ? rowId(activeIndex) : undefined}
    tabindex="0"
    data-testid="file-tree"
    bind:this={viewport}
    bind:clientHeight={height}
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    {onkeydown}
    {onfocus}
    {ondragstart}
    {ondragover}
    {ondrop}
    {ondragend}
    {ondragleave}
    {oncontextmenu}
    class:drop-root={dropDir === ""}
  >
    {#if error}
      <p class="note error" role="alert">{error}</p>
    {:else if rows.length === 0 && rootLoading}
      <p class="note">Loading files…</p>
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
            class:selected={chosenSet.has(row.path)}
            class:active={index === activeIndex}
            class:ignored={row.ignored}
            class:cut={isCut(row.path)}
            class:drop={dropDir === row.path}
            draggable={!row.draft && actions.edit === null}
            role="treeitem"
            aria-label={row.draft ? (row.kind === "dir" ? "New folder" : "New file") : row.name}
            tabindex="-1"
            aria-describedby={describe(row) ? `${rowId(index)}-d` : undefined}
            aria-level={row.depth + 1}
            aria-expanded={row.kind === "dir" ? row.expanded : undefined}
            aria-selected={chosenSet.has(row.path)}
            aria-posinset={row.posinset}
            aria-setsize={row.setsize}
            data-path={row.path}
            data-git={row.git ?? undefined}
            data-ignored={row.ignored || undefined}
            title={tooltip(row)}
            style:top="{index * ROW_HEIGHT}px"
            style:padding-left="{10 + row.depth * 16}px"
            onclick={(e) => onrowclick(e, row)}
            ondblclick={() => row.kind !== "dir" && row.git !== "deleted" && onkeep?.(row.path)}
          >
            <span class="twisty" class:open={row.expanded} aria-hidden="true"
              >{#if row.kind === "dir"}<Icon name="chevron-right" small />{/if}</span
            >
            {#if row.draft || (actions.edit?.kind === "rename" && actions.edit.path === row.path)}
              <NameInput
                value={row.name}
                range={stemRange(row.name, row.kind === "dir")}
                problem={actions.problem}
                label={row.draft ? (row.kind === "dir" ? "Folder name" : "File name") : "New name"}
                oncheck={(name) => actions.check(name)}
                oncommit={(name, blur) => void actions.commit(name, blur)}
                oncancel={() => actions.cancel()}
              />
            {:else}
              <span class="name git-{row.git ?? 'clean'}">{row.name}</span>
            {/if}
            {#if row.loading}<span class="hint" aria-hidden="true">…</span>{/if}
            {#if row.failed}<span class="failed" aria-hidden="true">!</span>{/if}
            {#if describe(row)}<span id="{rowId(index)}-d" class="visually-hidden">{describe(row)}</span>{/if}
            {#if row.git}
              <span class="dot git-{row.git}" aria-hidden="true" title={`${GIT_LETTER[row.git]}: ${describe(row)}`}
              ></span>
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
    background: var(--s1);
  }
  /* The shell's panel header: 44 px, the title 16 px in (Shell.svelte `.head`). */
  .toolbar {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    box-sizing: border-box;
    height: 44px;
    padding: 8px 8px 4px 16px;
  }

  .title {
    flex: 1;
    font-weight: 600;
  }
  /* Secondary chrome: muted until the panel is hovered or a button is focused. */
  .icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-3);
  }
  .panel:hover .icon,
  .icon:focus-visible {
    color: var(--text-2);
  }
  .icon:hover:not(:disabled),
  .icon[aria-pressed="true"] {
    background: var(--s3);
    color: var(--text);
  }
  .icon[aria-pressed="true"] {
    color: var(--accent);
  }
  .icon:disabled {
    opacity: 0.45;
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
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  .spacer {
    position: relative;
  }
  /* A 4 px inset, so the selection reads as a pill. 28 px from --ctl: the
     shell uses the name --row for its grid, which hides the token here. */
  .row {
    position: absolute;
    left: var(--sp-1);
    right: var(--sp-1);
    height: var(--ctl);
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 10px;
    border-radius: var(--r-xs);
    color: var(--text-2);
    white-space: nowrap;
    cursor: default;
    user-select: none;
  }
  .row:hover {
    background: var(--s3);
  }
  .row.selected {
    background: var(--sel);
    color: var(--text);
    font-weight: 500;
  }
  .viewport:focus-within .row.selected {
    background: var(--sel-focus);
  }
  /* Where a dragged file or folder would land. */
  .viewport .row.drop,
  .viewport.drop-root {
    background: var(--accent-soft);
  }
  .viewport:focus-visible .row.active {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  /* Cut and waiting for Paste. */
  .row.cut > :not(.dot) {
    opacity: 0.5;
  }
  .ignored .name {
    color: var(--text-2);
    font-style: italic;
  }
  /* A 12 px chevron in a 16 px column, turned down while the folder is open. */
  .twisty {
    display: grid;
    place-items: center;
    width: var(--icon-sm);
    flex: none;
    color: var(--text-2);
  }
  .twisty :global(svg) {
    width: 12px;
    height: 12px;
    transition: transform var(--dur-2) var(--ease-out);
  }
  .twisty.open :global(svg) {
    transform: rotate(90deg);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Git status as a 6 px dot in the change's colour: the letter in its
     tooltip, the word in the row's description. */
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    margin-right: var(--sp-1);
    border-radius: var(--r-pill);
    background: currentColor;
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
    color: var(--text-2);
  }
  .note {
    margin: 0;
    padding: 12px 14px;
  }
  .error,
  .failed {
    color: var(--danger);
  }
  .failed {
    flex: none;
    font-weight: 600;
  }
  /* Names keep the row's colour; the dot carries the status. */
  .dot.git-modified {
    color: var(--warn);
  }
  .dot.git-added,
  .dot.git-untracked {
    color: var(--ok);
  }
  .name.git-deleted {
    text-decoration: line-through;
  }
  .dot.git-deleted,
  .dot.git-conflicted {
    color: var(--danger);
  }
  .dot.git-renamed {
    color: var(--accent);
  }
  @media (forced-colors: active) {
    .row.selected {
      outline: 1px solid Highlight;
    }
  }
</style>
