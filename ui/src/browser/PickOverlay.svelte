<script lang="ts">
// Ovra's own boxes over the canvas: the element pick mode points at, and the
// ones picked on this page, numbered. Drawn by Ovra, never inside the page, so
// a page can neither fake nor hide them.
import type { FrameMetadata, PageInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { type Fit, toCanvas } from "./geometry";
import type { Picker } from "./picker.svelte";
import { browserPicks, type PendingPick, pickLabel } from "./picks.svelte";

let { page, picker, frame, meta }: { page: PageInfo; picker: Picker; frame: Fit; meta: FrameMetadata } = $props();

const picks = browserPicks(useOvra().transport);
const mine = $derived(picks.of(page.session).filter((p) => p.page === page.id));
const hovered = $derived(picker.active && picker.hovered ? toCanvas(picker.hovered.rect, frame) : null);

/** Where a pick is now: its box moved by the page's scroll since it was picked. */
function placed(p: PendingPick) {
  const { rect } = p.pick;
  return toCanvas({ ...rect, x: rect.x + p.scroll.x - meta.scroll_x, y: rect.y + p.scroll.y - meta.scroll_y }, frame);
}
</script>

<div class="overlay" aria-hidden="true">
  {#each mine as p, i (p.pick.pick_id)}
    {@const at = placed(p)}
    <div
      class="box picked"
      style:left="{at.x}px"
      style:top="{at.y}px"
      style:width="{at.width}px"
      style:height="{at.height}px"
      data-testid="browser-picked-box"
    >
      <span class="label">{i + 1}</span>
    </div>
  {/each}
  {#if hovered && picker.hovered}
    <div
      class="box hover"
      style:left="{hovered.x}px"
      style:top="{hovered.y}px"
      style:width="{hovered.width}px"
      style:height="{hovered.height}px"
      data-testid="browser-hover-box"
    >
      <span class="label">{pickLabel(picker.hovered)}</span>
    </div>
  {/if}
</div>

<style>
  .overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .box {
    position: absolute;
    box-sizing: border-box;
  }
  .hover {
    background: var(--accent-soft);
    border: 2px solid var(--accent);
  }
  .picked {
    border: 2px dashed var(--ok);
  }
  .label {
    position: absolute;
    top: -20px;
    left: -2px;
    padding: 0 4px;
    max-width: 320px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 12px;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .picked .label {
    background: var(--ok);
  }
  @media (forced-colors: active) {
    .box {
      border-color: Highlight;
    }
  }
</style>
