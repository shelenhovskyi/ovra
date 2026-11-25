<script lang="ts">
// The handle between a side panel and the main area: drag it, or focus it and
// use the arrow keys (Home and End for the limits); a double click resets.
// `edge` is the side of the window the panel sits on, so dragging away from
// that edge widens it.
import { MIN_WIDTH } from "./layout.svelte";

const STEP = 16;

let {
  label,
  width,
  max,
  edge,
  onresize,
  onreset,
}: {
  label: string;
  width: number;
  /** The widest the panel may be now. */
  max: number;
  edge: "left" | "right";
  onresize: (width: number) => void;
  onreset: () => void;
} = $props();

let drag = $state<{ x: number; width: number } | null>(null);

function onpointerdown(event: PointerEvent): void {
  if (event.button !== 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  drag = { x: event.clientX, width };
}

function onpointermove(event: PointerEvent): void {
  if (!drag) return;
  const moved = event.clientX - drag.x;
  onresize(drag.width + (edge === "left" ? moved : -moved));
}

function onkeydown(event: KeyboardEvent): void {
  const grow = edge === "left" ? "ArrowRight" : "ArrowLeft";
  const shrink = edge === "left" ? "ArrowLeft" : "ArrowRight";
  const next =
    event.code === grow
      ? width + STEP
      : event.code === shrink
        ? width - STEP
        : event.code === "Home"
          ? MIN_WIDTH
          : event.code === "End"
            ? max
            : null;
  if (next === null) return;
  event.preventDefault();
  onresize(next);
}
</script>

<!-- A focusable separator is a WAI-ARIA window splitter, a widget. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="handle"
  class:active={drag !== null}
  role="separator"
  tabindex="0"
  aria-label={label}
  aria-orientation="vertical"
  aria-valuenow={width}
  aria-valuemin={MIN_WIDTH}
  aria-valuemax={max}
  {onpointerdown}
  {onpointermove}
  onpointerup={() => (drag = null)}
  onpointercancel={() => (drag = null)}
  ondblclick={onreset}
  {onkeydown}
></div>
{#if drag}<div class="dragging" aria-hidden="true"></div>{/if}

<style>
  .handle {
    position: relative;
    width: 100%;
    height: 100%;
    cursor: col-resize;
    touch-action: none;
  }
  /* A thin line on hover or focus; a grip in the middle. */
  .handle::before {
    content: "";
    position: absolute;
    inset: 0 calc(50% - 1px);
    border-radius: 1px;
  }
  .handle::after {
    content: "";
    position: absolute;
    top: calc(50% - 14px);
    left: calc(50% - 2px);
    width: 4px;
    height: 28px;
    border-radius: 2px;
    background: var(--s4);
  }
  .handle:hover::before,
  .handle:focus-visible::before,
  .active::before,
  .handle:hover::after,
  .active::after {
    background: var(--accent);
  }
  .handle:focus-visible {
    outline: none;
  }
  /* While dragging, the whole window shows the resize cursor and selects no text. */
  .dragging {
    position: fixed;
    inset: 0;
    z-index: 300;
    cursor: col-resize;
  }
  @media (forced-colors: active) {
    .handle:focus-visible::before,
    .handle:hover::before {
      background: Highlight;
    }
  }
</style>
