<script lang="ts" module>
import type { ISearchOptions, ISearchResultChangeEvent } from "@xterm/addon-search";

/** The part of xterm's search addon the bar uses. */
export type TerminalSearch = {
  findNext(term: string, options?: ISearchOptions): boolean;
  findPrevious(term: string, options?: ISearchOptions): boolean;
  clearDecorations(): void;
  onDidChangeResults(listener: (event: ISearchResultChangeEvent) => void): { dispose(): void };
};
</script>

<script lang="ts">
// Find in one terminal's output and scrollback: over the terminal's top
// right corner, like the editor's find panel. Enter finds the next match,
// Shift+Enter the previous one, Escape closes the bar.
import { onDestroy } from "svelte";
import Icon from "../shell/Icon.svelte";

let {
  search,
  decorations,
  onclose,
}: {
  search: TerminalSearch;
  /** How matches are highlighted; also what makes the addon count them. */
  decorations: ISearchOptions["decorations"];
  /** Escape or the close button: the view hides the bar and focuses the terminal. */
  onclose: () => void;
} = $props();

let input: HTMLInputElement | undefined = $state();
let query = $state("");
let results = $state<ISearchResultChangeEvent | null>(null);

$effect(() => {
  const sub = search.onDidChangeResults((event) => (results = event));
  return () => sub.dispose();
});
onDestroy(() => search.clearDecorations());

/** Focuses the search text with it selected. */
export function focus(): void {
  input?.focus();
  input?.select();
}

function find(backwards: boolean, incremental = false): void {
  if (!query) return;
  const options = { decorations, incremental };
  if (backwards) search.findPrevious(query, options);
  else search.findNext(query, options);
}

function changed(): void {
  if (query) return find(false, true);
  search.clearDecorations();
  results = null;
}

function onkeydown(event: KeyboardEvent): void {
  if (event.key === "Enter") {
    event.preventDefault();
    find(event.shiftKey);
  } else if (event.key === "Escape") {
    event.preventDefault();
    onclose();
  }
}

const count = $derived.by(() => {
  if (!query || !results) return "";
  if (results.resultCount === 0) return "No results";
  // -1: more matches than the addon highlights.
  if (results.resultIndex < 0) return `${results.resultCount}+ results`;
  return `${results.resultIndex + 1} of ${results.resultCount}`;
});
</script>

<div class="find" role="search" aria-label="Find in terminal">
  <input
    type="text"
    placeholder="Find"
    aria-label="Find in terminal"
    spellcheck="false"
    autocomplete="off"
    bind:this={input}
    bind:value={query}
    oninput={changed}
    {onkeydown}
  />
  <span class="count" aria-live="polite">{count}</span>
  <button type="button" class="previous" aria-label="Previous match" title="Previous match (Shift+Enter)" onclick={() => find(true)}
    ><Icon name="chevron" small /></button
  >
  <button type="button" aria-label="Next match" title="Next match (Enter)" onclick={() => find(false)}
    ><Icon name="chevron" small /></button
  >
  <button type="button" aria-label="Close find" title="Close (Escape)" onclick={onclose}><Icon name="close" small /></button>
</div>

<style>
  .find {
    position: absolute;
    top: 6px;
    right: 14px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-radius: var(--radius);
    background: var(--s2);
    box-shadow: var(--card-shadow);
    color: var(--text-2);
  }
  input {
    width: 180px;
    padding: 4px 8px;
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .count {
    min-width: 0;
    padding: 0 6px;
    white-space: nowrap;
    font-size: 12px;
  }
  .count:empty {
    padding: 0;
  }
  button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    color: inherit;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
    color: var(--text);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .previous :global(svg) {
    transform: rotate(180deg);
  }
  @media (forced-colors: active) {
    .find {
      border: 1px solid CanvasText;
    }
  }
</style>
