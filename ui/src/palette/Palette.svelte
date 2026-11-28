<script lang="ts">
// The palette: fuzzy search over commands, the project's files or the known
// projects (see `mode.svelte.ts`), or a folder path to open. The shell mounts it while
// `workspace.paletteOpen` is true; it closes by setting it to false.
import { tick } from "svelte";
import { focusMain } from "../editor/open-files.svelte";
import { runCommand } from "../keys/keybindings";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import FileIcon from "../shell/file-icons/FileIcon.svelte";
import type { TextSpot } from "../shell/workspace.svelte";
import { FolderOpener } from "../tree/folder-opener.svelte";
import { focusStartScreen } from "./commands";
import { type Ranked, segments } from "./fuzzy";
import { splitLine, spotLabel } from "./line-suffix";
import { palette } from "./mode.svelte";
import { createRanker, type Ranker } from "./ranker";
import { keywordMatches, noteRecent } from "./search";
import { type Entry, source } from "./sources";

const { transport, workspace } = useOvra();
// svelte-ignore state_referenced_locally
const from = source(palette.mode, transport, workspace);
// Folder mode: the typed path opens as a project, through the same checks as the start screen.
// svelte-ignore state_referenced_locally
const folder =
  palette.mode === "folder"
    ? new FolderOpener(transport, (project) => {
        workspace.openPath = null;
        workspace.mode = "view";
        workspace.project = project;
      })
    : null;
const apple = isApple();
const browser = inBrowserTab();

type Pool = { group: string | undefined; entries: Entry[]; ranker: Ranker };

let entries = $state.raw<Entry[] | null>(null);
let pools = $state.raw<Pool[] | null>(null);
let failure = $state<string | null>(null);
let query = $state("");
let selected = $state(0);
let input: HTMLInputElement | undefined = $state();
let list: HTMLUListElement | undefined = $state();
/** Rows for `rankedQuery`; ranking may lag one keystroke behind `query`. */
let items = $state<Ranked<Entry>[]>([]);
let matches = $state(0);
let rankedQuery = $state<string | null>(null);
/** The line `rankedQuery` ends in (`main.rs:42`): the files picked open there. */
let rankedSpot = $state<TextSpot | null>(null);
/** Enter pressed before the rows for the current query arrived: run the top row when they do. */
let enterPending = false;
const announcement = $derived(
  failure ??
    (rankedQuery === null
      ? "Loading"
      : matches === 0
        ? from.empty
        : `${matches} ${matches === 1 ? "result" : "results"}`),
);

$effect(() => {
  let live = true;
  Promise.resolve(from.entries()).then(
    (loaded) => {
      if (!live) return;
      entries = loaded;
      // One ranker per group, so many files never push the commands out of the rows.
      const groups = [...new Set(loaded.map((e) => e.group))];
      pools = groups.map((group) => {
        const members = loaded.filter((e) => e.group === group);
        return { group, entries: members, ranker: createRanker(members.map((e) => e.label)) };
      });
    },
    (error: unknown) => {
      if (live) failure = error instanceof Error ? error.message : String(error);
    },
  );
  return () => {
    live = false;
  };
});

$effect(() => {
  const current = pools;
  return () => {
    for (const pool of current ?? []) pool.ranker.dispose();
  };
});

/** Rows for `wanted` from each group, ranked, then the entries only a keyword matches. */
async function rankPools(
  current: Pool[],
  wanted: string,
): Promise<{ rows: Ranked<Entry>[]; matches: number; spot: TextSpot | null }> {
  const idle = from.idle;
  const commandsOnly = from.prefix === true && wanted.startsWith(">");
  const { text: query, spot } = from.lines && !commandsOnly ? splitLine(wanted) : { text: wanted, spot: null };
  // A bare `:42`: the line in the file shown.
  if (spot && query.trim() === "") {
    const shown = entries?.find((e) => e.openAt && e.id === `file:${workspace.openPath}`);
    const rows = shown ? [{ item: shown, positions: [] }] : [];
    return { rows, matches: rows.length, spot };
  }
  if (idle && entries && query.trim() === "") {
    const rows = idle(entries).map((item) => ({ item, positions: [] }));
    return { rows, matches: rows.length, spot: null };
  }
  const text = commandsOnly ? wanted.slice(1).trim() : query;
  const ranked = await Promise.all(
    current
      .filter((pool) => !commandsOnly || pool.group === "Commands")
      .map(async (pool) => {
        const result = await pool.ranker.rank(text, from.limit);
        const rows = result.rows.flatMap(({ index, positions }) => {
          const item = pool.entries[index];
          return item ? [{ item, positions }] : [];
        });
        const extra = keywordMatches(pool.entries, text, new Set(rows.map((row) => row.item.id)));
        const room = Math.max(0, from.limit - rows.length);
        return {
          rows: [...rows, ...extra.slice(0, room).map((item) => ({ item, positions: [] }))],
          matches: result.matches + extra.length,
        };
      }),
  );
  return { rows: ranked.flatMap((r) => r.rows), matches: ranked.reduce((sum, r) => sum + r.matches, 0), spot };
}

$effect(() => {
  const current = pools;
  const wanted = query;
  if (!current) return;
  let live = true;
  void rankPools(current, wanted).then((result) => {
    if (!live) return;
    items = result.rows;
    matches = result.matches;
    rankedQuery = wanted;
    rankedSpot = result.spot;
    if (enterPending) {
      enterPending = false;
      run(items[selected]);
    }
  });
  return () => {
    live = false;
  };
});

$effect(() => {
  const previous = document.activeElement;
  input?.focus();
  return () => {
    // A command may have opened something that took focus, or switched the
    // palette to another mode (the effect runs again): leave focus there.
    queueMicrotask(() => {
      if (workspace.paletteOpen) return;
      const now = document.activeElement;
      if (now && now !== document.body && now.isConnected) return;
      if (previous instanceof HTMLElement && previous.isConnected && previous !== document.body) previous.focus();
      // Opened with focus nowhere, on the start screen: back to its first project rather than the page.
      else if (!workspace.project) void focusStartScreen();
      // Otherwise the open file, so Escape never leaves focus on the page.
      else void focusMain();
    });
  };
});

$effect(() => {
  list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView?.({ block: "nearest" });
});

function close(): void {
  workspace.paletteOpen = false;
  palette.mode = "commands";
  palette.list = null;
}

function run(item: Ranked<Entry> | undefined): void {
  if (!item) return;
  // A handed-over list (the symbols of a file) is not a place to come back to.
  const recent = palette.mode !== "list";
  close();
  // Run once the palette is gone, so a command can open it again in another mode.
  const { id, label, openAt } = item.item;
  const spot = rankedSpot;
  const run = spot && openAt ? () => openAt(spot) : item.item.run;
  if (recent) noteRecent(id);
  void tick().then(() => runCommand({ id, title: label, run }));
}

/** Opens the typed folder; a refused path stays in the box with the reason. */
async function openTyped(): Promise<void> {
  if (folder && (await folder.open(query.trim()))) close();
}

/** Rows PageUp and PageDown move by. */
const PAGE = 10;

/** Ctrl+N/Ctrl+J and Ctrl+P/Ctrl+K move like the arrows (Emacs, fzf), on any layout. */
const CTRL_MOVES: Record<string, string> = { KeyN: "ArrowDown", KeyJ: "ArrowDown", KeyP: "ArrowUp", KeyK: "ArrowUp" };

function movement(event: KeyboardEvent): string {
  const ctrlOnly = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
  return (ctrlOnly && CTRL_MOVES[event.code]) || event.key;
}

function onkeydown(event: KeyboardEvent): void {
  const count = items.length;
  switch (movement(event)) {
    case "ArrowDown":
      if (count) selected = (selected + 1) % count;
      break;
    case "ArrowUp":
      if (count) selected = (selected - 1 + count) % count;
      break;
    case "PageDown":
      if (count) selected = Math.min(selected + PAGE, count - 1);
      break;
    case "PageUp":
      if (count) selected = Math.max(selected - PAGE, 0);
      break;
    case "Enter":
      if (folder) void openTyped();
      else if (rankedQuery === query) run(items[selected]);
      else enterPending = failure === null;
      break;
    case "Escape":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
}

function onfocusout(event: FocusEvent): void {
  const next = event.relatedTarget;
  if (!(next instanceof Node && event.currentTarget instanceof Node && event.currentTarget.contains(next))) close();
}
</script>

<div class="backdrop">
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette" data-commands="palette." {onfocusout}>
    <input
      bind:this={input}
      bind:value={query}
      oninput={() => {
        selected = 0;
        if (folder) folder.error = null;
      }}
      {onkeydown}
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
      aria-activedescendant={items.length ? `palette-item-${selected}` : undefined}
      aria-label={from.placeholder}
      aria-invalid={folder?.error ? true : undefined}
      placeholder={from.placeholder}
      spellcheck="false"
      autocomplete="off"
    />
    <ul id="palette-list" role="listbox" aria-label={from.name} bind:this={list}>
      {#each items as item, index (item.item.id)}
        {#if item.item.group && item.item.group !== items[index - 1]?.item.group}
          <li class="group" role="presentation">{item.item.group}</li>
        {/if}
        <li
          id="palette-item-{index}"
          data-index={index}
          role="option"
          aria-selected={index === selected}
          onmousedown={(event) => event.preventDefault()}
          onmousemove={() => (selected = index)}
          onclick={() => run(item)}
          onkeydown={() => {}}
        >
          <span class="title">
            {#if item.item.id.startsWith("file:")}<FileIcon
                name={item.item.label.slice(item.item.label.lastIndexOf("/") + 1)}
              />{/if}
            {#if item.item.category}<span class="category">{item.item.category}</span>{/if}
            {#each segments(item.item.label, item.positions) as part}
              {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
            {/each}
            {#if item.item.detail}<span class="detail">{item.item.detail}</span>{/if}
            {#if rankedSpot && item.item.openAt}<span class="detail">{spotLabel(rankedSpot)}</span>{/if}
          </span>
          {#if item.item.keys}<kbd>{formatShortcut(liveKeys(item.item.keys, browser), apple)}</kbd>{/if}
        </li>
      {:else}
        {#if folder}
          <li class="empty" class:error={folder.error} role="presentation">{folder.error ?? from.empty}</li>
        {:else}
          <li class="empty" role="presentation">{failure ?? (rankedQuery === null ? "Loading…" : from.empty)}</li>
        {/if}
      {/each}
    </ul>
    {#if matches > items.length}
      <p class="more">Showing {items.length} of {matches.toLocaleString("en")}. Type more to narrow down.</p>
    {/if}
    <p class="visually-hidden" role="status">{folder ? (folder.error ?? "") : query || failure ? announcement : ""}</p>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 18vh;
    background: var(--scrim);
    z-index: 100;
  }
  .palette {
    width: min(640px, calc(100vw - 32px));
    padding: var(--sp-2);
    background: var(--menu);
    backdrop-filter: var(--menu-blur);
    border-radius: var(--r-xl);
    box-shadow: var(--card-shadow);
    overflow: hidden;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    height: 42px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-md);
    background: var(--s3);
    color: inherit;
    font: inherit;
    font-size: 15px;
    outline: none;
  }
  ul {
    max-height: 50vh;
    margin: 0;
    padding: 6px 0 0;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    min-height: 36px;
    padding: 6px 12px;
    border-radius: var(--r-md);
    cursor: default;
  }
  li[aria-selected="true"] {
    background: var(--sel-focus);
  }
  mark {
    background: none;
    color: var(--accent);
    font-weight: 600;
  }
  kbd {
    padding: 3px 6px;
    font: 500 12px / 1 var(--font);
    color: var(--text-2);
    background: var(--s3);
    border-radius: var(--r-xs);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title :global(.file-icon) {
    display: inline-block;
    margin-right: 8px;
    vertical-align: -3px;
  }
  .detail {
    margin-left: 8px;
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
  .category {
    display: inline-block;
    min-width: 64px;
    margin-right: 8px;
    font: var(--t-small);
    color: var(--text-2);
  }
  .group {
    min-height: 0;
    padding: 8px 12px 4px;
    font: var(--t-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  /* Forced colours (Windows High Contrast) drop backgrounds and text colours,
     so the selection gets a system-coloured outline and matches an underline. */
  @media (forced-colors: active) {
    .palette {
      border: 1px solid CanvasText;
    }
    li[aria-selected="true"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
      background: Highlight;
      color: HighlightText;
      forced-color-adjust: none;
    }
    li[aria-selected="true"] mark,
    li[aria-selected="true"] .detail,
    li[aria-selected="true"] kbd {
      color: HighlightText;
    }
    mark {
      text-decoration: underline;
      text-underline-offset: 2px;
    }
  }
  .empty.error {
    color: var(--danger);
  }
  .more {
    margin: 0;
    padding: 8px 12px 4px;
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
  .empty {
    color: var(--text-2);
    cursor: default;
  }
</style>
