<script lang="ts">
let { src, path }: { src: string; path: string } = $props();

const name = $derived(path.slice(path.lastIndexOf("/") + 1));

let size = $state<{ width: number; height: number } | null>(null);
let failed = $state(false);
// Tiny images (icons, 1 × 1 pixels) are enlarged by whole steps so they can be seen.
const scale = $derived(size ? Math.max(1, Math.floor(64 / Math.max(size.width, size.height, 1))) : 1);

function onload(event: Event) {
  const img = event.currentTarget as HTMLImageElement;
  size = { width: img.naturalWidth, height: img.naturalHeight };
}
</script>

<figure class="image" data-testid="image-view" aria-label="Image {name}">
  {#if failed}
    <p class="note">This image cannot be shown.</p>
  {:else}
    <img
      {src}
      alt={name}
      {onload}
      onerror={() => (failed = true)}
      class:pixelated={scale > 1}
      style:width={size && scale > 1 ? `${size.width * scale}px` : null}
    />
  {/if}
  {#if size}<figcaption>{name}, {size.width} × {size.height} pixels{scale > 1 ? `, shown ${scale}× larger` : ""}</figcaption>{/if}
</figure>

<style>
  .image {
    margin: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 12px;
    box-sizing: border-box;
    /* Checkerboard so transparent pixels are visible. */
    background: repeating-conic-gradient(var(--s3) 0% 25%, var(--s1) 0% 50%) 0 0 / 16px 16px;
  }
  img {
    max-width: 100%;
    max-height: calc(100% - 24px);
    object-fit: contain;
  }
  .pixelated {
    image-rendering: pixelated;
  }
  figcaption,
  .note {
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
</style>
