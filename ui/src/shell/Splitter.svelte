<script lang="ts">
// The handle between a side panel and the main area: drag it, or focus it and
// use the arrow keys (Home and End for the limits); a double click resets.
// `edge` is the side of the window the panel sits on, so dragging away from
// that edge widens it. With `edge="bottom"` the handle lies across the top of
// a panel at the bottom and sets its height: up and down instead of left and right.
import { MIN_WIDTH } from "./layout.svelte";

const STEP = 16;

let {
  label,
  width,
  max,
  min = MIN_WIDTH,
  edge,
  onresize,
  onreset,
}: {
  label: string;
  width: number;
  /** The widest (or tallest) the panel may be now. */
  max: number;
  /** The narrowest it may be. */
  min?: number;
  edge: "left" | "right" | "bottom";
  onresize: (width: number) => void;
  onreset: () => void;
} = $props();

const vertical = $derived(edge === "bottom");
let drag = $state<{ x: number; width: number } | null>(null);

function along(event: PointerEvent): number {
  return vertical ? event.clientY : event.clientX;
}

function onpointerdown(event: PointerEvent): void {
  if (event.button !== 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  drag = { x: along(event), width };
}

function onpointermove(event: PointerEvent): void {
  if (!drag) return;
  // The button came up where the handle did not see it (capture was lost).
  if (event.buttons === 0) {
    drag = null;
    return;
  }
  const moved = along(event) - drag.x;
  onresize(drag.width + (edge === "left" ? moved : -moved));
}

function onkeydown(event: KeyboardEvent): void {
  const grow = { left: "ArrowRight", right: "ArrowLeft", bottom: "ArrowUp" }[edge];
  const shrink = { left: "ArrowLeft", right: "ArrowRight", bottom: "ArrowDown" }[edge];
  const next =
    event.code === grow
      ? width + STEP
      : event.code === shrink
        ? width - STEP
        : event.code === "Home"
          ? min
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
  class="handle {edge}"
  class:active={drag !== null}
  role="separator"
  tabindex="0"
  aria-label={label}
  aria-orientation={vertical ? "horizontal" : "vertical"}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  {onpointerdown}
  {onpointermove}
  onpointerup={() => (drag = null)}
  onpointercancel={() => (drag = null)}
  onlostpointercapture={() => (drag = null)}
  ondblclick={onreset}
  {onkeydown}
></div>
<!-- Events reach the overlay only without capture, e.g. after the panel was hidden mid-drag. -->
{#if drag}<div
    class="dragging"
    class:rows={vertical}
    aria-hidden="true"
    {onpointermove}
    onpointerup={() => (drag = null)}
    onpointerdown={() => (drag = null)}
  ></div>{/if}

<style>
  .handle {
    position: relative;
    width: 100%;
    height: 100%;
    cursor: col-resize;
    touch-action: none;
  }
  /* A hairline on the side that faces the main area marks where the panel
     ends; hover, focus and dragging thicken it to the accent and show a grip. */
  .handle::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
  }
  .left::before {
    right: 0;
  }
  .right::before {
    left: 0;
  }
  .bottom {
    cursor: row-resize;
  }
  /* Across the middle of the handle, which the panel centres on its top edge. */
  .bottom::before {
    top: calc(50% - 0.5px);
    bottom: auto;
    left: 0;
    right: 0;
    width: auto;
    height: 1px;
  }
  .handle::after {
    content: "";
    position: absolute;
    top: calc(50% - var(--ctl) / 2);
    left: calc(50% - 2px);
    width: 4px;
    height: var(--ctl);
    border-radius: var(--r-control);
    opacity: 0;
    background: var(--accent);
    transition: opacity var(--dur-1) var(--ease-out);
  }
  .handle:hover::before,
  .handle:focus-visible::before,
  .active::before {
    width: 2px;
    background: var(--accent);
  }
  .bottom:hover::before,
  .bottom:focus-visible::before,
  .bottom.active::before {
    top: calc(50% - 1px);
    width: auto;
    height: 2px;
  }
  .bottom::after {
    top: calc(50% - 2px);
    left: calc(50% - 14px);
    width: 28px;
    height: 4px;
  }
  .handle:hover::after,
  .handle:focus-visible::after,
  .active::after {
    opacity: 1;
  }
  .handle:focus-visible {
    outline: none;
  }
  /* Forced colors replace the accent fills: the hairline and grip take system
     colours, and focus shows as an outline, as on every other control. */
  @media (forced-colors: active) {
    .handle::before,
    .handle::after {
      forced-color-adjust: none;
      background: CanvasText;
    }
    .handle:hover::before,
    .handle:focus-visible::before,
    .active::before,
    .handle::after {
      background: Highlight;
    }
    .handle:focus-visible {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
  /* While dragging, the whole window shows the resize cursor and selects no text. */
  .dragging {
    position: fixed;
    inset: 0;
    z-index: 300;
    cursor: col-resize;
  }
  .dragging.rows {
    cursor: row-resize;
  }
  @media (forced-colors: active) {
    .handle:focus-visible::before,
    .handle:hover::before {
      background: Highlight;
    }
  }
</style>
