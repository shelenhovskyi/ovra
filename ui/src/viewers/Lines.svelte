<script lang="ts" generics="T">
// A scrollable table of fixed-height rows. Long lists render only the rows on
// screen (see lines.ts); rows must not wrap. Every rendered row carries its
// position (aria-rowindex of aria-rowcount), and keyboard scrolling announces
// which lines are on screen, so screen readers know how much there is.
import type { Snippet } from "svelte";
import { visibleRange } from "./lines";

let {
  items,
  row,
  label,
  rowHeight = 22,
  testid,
  onkeydown,
}: {
  items: readonly T[];
  row: Snippet<[T, number]>;
  label: string;
  rowHeight?: number;
  testid?: string;
  onkeydown?: (event: KeyboardEvent) => void;
} = $props();

let viewport = $state<HTMLElement | null>(null);
let scrollTop = $state(0);
let height = $state(0);
let status = $state("");
let announceScroll = false;

const range = $derived(visibleRange(items.length, scrollTop, height, rowHeight));
const shown = $derived(items.slice(range.start, range.end));

const SCROLL_KEYS = new Set(["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown"]);

/** Scrolls so that row `index` is at the top. */
export function scrollToIndex(index: number): void {
  if (!viewport) return;
  viewport.scrollTop = index * rowHeight;
  scrollTop = viewport.scrollTop;
}

/** Moves keyboard focus to the list. */
export function focus(): void {
  viewport?.focus();
}

/** Says `message` through the list's polite live region. */
export function announce(message: string): void {
  status = message;
}

function onscroll() {
  if (!viewport) return;
  scrollTop = viewport.scrollTop;
  if (!announceScroll) return;
  announceScroll = false;
  const first = Math.min(items.length, Math.floor(scrollTop / rowHeight) + 1);
  const last = Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight));
  status = `Lines ${first}–${Math.max(first, last)} of ${items.length}`;
}
</script>

<div class="lines">
  <!-- A focusable scroll region, so the keyboard can scroll it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="viewport"
    role="table"
    aria-label={label}
    aria-rowcount={items.length}
    tabindex="0"
    data-testid={testid}
    bind:this={viewport}
    bind:clientHeight={height}
    {onscroll}
    onkeydown={(event) => {
      if (SCROLL_KEYS.has(event.key)) announceScroll = true;
      onkeydown?.(event);
    }}
  >
    <div
      class="sizer"
      style:height="{items.length * rowHeight}px"
      style:padding-top="{range.start * rowHeight}px"
      style:--row="{rowHeight}px"
    >
      {#each shown as item, i (range.start + i)}
        <div class="row" role="row" aria-rowindex={range.start + i + 1}>
          <div role="cell">{@render row(item, range.start + i)}</div>
        </div>
      {/each}
    </div>
  </div>
  <p class="visually-hidden" role="status">{status}</p>
</div>

<style>
  .lines {
    height: 100%;
    min-height: 0;
  }
  .viewport {
    height: 100%;
    overflow: auto;
    font-family: var(--mono);
    font-size: var(--fs-ui);
  }
  .viewport:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .sizer {
    box-sizing: border-box;
    min-width: 100%;
    width: max-content;
  }
  .row {
    height: var(--row);
    line-height: var(--row);
    white-space: pre;
    overflow: hidden;
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
</style>
