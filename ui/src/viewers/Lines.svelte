<script lang="ts" generics="T">
// A scrollable list of fixed-height rows. Long lists render only the rows on
// screen (see lines.ts); rows must not wrap.
import type { Snippet } from "svelte";
import { visibleRange } from "./lines";

let {
  items,
  row,
  label,
  rowHeight = 18,
  testid,
}: { items: readonly T[]; row: Snippet<[T, number]>; label: string; rowHeight?: number; testid?: string } = $props();

let viewport = $state<HTMLElement | null>(null);
let scrollTop = $state(0);
let height = $state(0);

const range = $derived(visibleRange(items.length, scrollTop, height, rowHeight));
const shown = $derived(items.slice(range.start, range.end));

/** Scrolls so that row `index` is at the top. */
export function scrollToIndex(index: number): void {
  if (!viewport) return;
  viewport.scrollTop = index * rowHeight;
  scrollTop = viewport.scrollTop;
}
</script>

<!-- A focusable scroll region, so the keyboard can scroll it. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="lines"
  role="region"
  aria-label={label}
  tabindex="0"
  data-testid={testid}
  bind:this={viewport}
  bind:clientHeight={height}
  onscroll={() => {
    if (viewport) scrollTop = viewport.scrollTop;
  }}
>
  <div
    class="sizer"
    style:height="{items.length * rowHeight}px"
    style:padding-top="{range.start * rowHeight}px"
    style:--row="{rowHeight}px"
  >
    {#each shown as item, i (range.start + i)}
      {@render row(item, range.start + i)}
    {/each}
  </div>
</div>

<style>
  .lines {
    height: 100%;
    overflow: auto;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  .lines:focus-visible {
    outline: 2px solid #7aa2f7;
    outline-offset: -2px;
  }
  .sizer {
    box-sizing: border-box;
    min-width: 100%;
    width: max-content;
  }
  .sizer > :global(*) {
    height: var(--row);
    line-height: var(--row);
    white-space: pre;
    overflow: hidden;
  }
</style>
