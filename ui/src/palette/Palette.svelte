<script lang="ts">
// The palette: fuzzy search over commands, the project's files or the known
// projects (see `mode.svelte.ts`). The shell mounts it while
// `workspace.paletteOpen` is true; it closes by setting it to false.
import { tick } from "svelte";
import { runCommand } from "../keys/keybindings";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import { type Ranked, segments } from "./fuzzy";
import { palette } from "./mode.svelte";
import { createRanker, type Ranker } from "./ranker";
import { type Entry, source } from "./sources";

const { transport, workspace } = useOvra();
// svelte-ignore state_referenced_locally
const from = source(palette.mode, transport, workspace);
const apple = isApple();
const browser = inBrowserTab();

let entries = $state<Entry[] | null>(null);
let ranker = $state<Ranker | null>(null);
let failure = $state<string | null>(null);
let query = $state("");
let selected = $state(0);
let input: HTMLInputElement | undefined = $state();
let list: HTMLUListElement | undefined = $state();
/** Rows for `rankedQuery`; ranking may lag one keystroke behind `query`. */
let items = $state<Ranked<Entry>[]>([]);
let matches = $state(0);
let rankedQuery = $state<string | null>(null);
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
      ranker = createRanker(loaded.map((e) => e.label));
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
  const current = ranker;
  return () => current?.dispose();
});

$effect(() => {
  const current = ranker;
  const loaded = entries;
  const wanted = query;
  if (!current || !loaded) return;
  let live = true;
  void current.rank(wanted, from.limit).then((result) => {
    if (!live) return;
    items = result.rows.flatMap(({ index, positions }) => {
      const item = loaded[index];
      return item ? [{ item, positions }] : [];
    });
    matches = result.matches;
    rankedQuery = wanted;
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
    if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
  };
});

$effect(() => {
  list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView?.({ block: "nearest" });
});

function close(): void {
  workspace.paletteOpen = false;
  palette.mode = "commands";
}

function run(item: Ranked<Entry> | undefined): void {
  if (!item) return;
  close();
  // Run once the palette is gone, so a command can open it again in another mode.
  const { id, label, run } = item.item;
  void tick().then(() => runCommand({ id, title: label, run }));
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
      if (rankedQuery === query) run(items[selected]);
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
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette" {onfocusout}>
    <input
      bind:this={input}
      bind:value={query}
      oninput={() => (selected = 0)}
      {onkeydown}
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
      aria-activedescendant={items.length ? `palette-item-${selected}` : undefined}
      aria-label={from.placeholder}
      placeholder={from.placeholder}
      spellcheck="false"
      autocomplete="off"
    />
    <ul id="palette-list" role="listbox" aria-label={from.name} bind:this={list}>
      {#each items as item, index (item.item.id)}
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
            {#each segments(item.item.label, item.positions) as part}
              {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
            {/each}
            {#if item.item.detail}<span class="detail">{item.item.detail}</span>{/if}
          </span>
          {#if item.item.keys}<kbd>{formatShortcut(liveKeys(item.item.keys, browser), apple)}</kbd>{/if}
        </li>
      {:else}
        <li class="empty" role="presentation">{failure ?? (rankedQuery === null ? "Loading…" : from.empty)}</li>
      {/each}
    </ul>
    {#if matches > items.length}
      <p class="more">Showing {items.length} of {matches.toLocaleString("en")}. Type more to narrow down.</p>
    {/if}
    <p class="visually-hidden" role="status">{query || failure ? announcement : ""}</p>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.35);
    z-index: 100;
  }
  .palette {
    width: min(600px, calc(100vw - 32px));
    background: #1e1e24;
    border: 1px solid #34343d;
    border-radius: 8px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
    overflow: hidden;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    padding: 10px 12px;
    border: 0;
    border-bottom: 1px solid #2a2a31;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  ul {
    max-height: 50vh;
    margin: 0;
    padding: 4px 0;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    cursor: pointer;
  }
  li[aria-selected="true"] {
    background: #2d3a5c;
  }
  mark {
    background: none;
    color: #7aa2ff;
    font-weight: 600;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    color: #9a9aa6;
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    margin-left: 8px;
    color: #9a9aa6;
    font-size: 12px;
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
  .more {
    margin: 0;
    padding: 6px 12px;
    border-top: 1px solid #2a2a31;
    color: #9a9aa6;
    font-size: 12px;
  }
  .empty {
    color: #9a9aa6;
    cursor: default;
  }
</style>
