<script lang="ts">
let { src, path }: { src: string; path: string } = $props();

const name = $derived(path.slice(path.lastIndexOf("/") + 1));

let size = $state<{ width: number; height: number } | null>(null);
let failed = $state(false);

function onload(event: Event) {
  const img = event.currentTarget as HTMLImageElement;
  size = { width: img.naturalWidth, height: img.naturalHeight };
}
</script>

<figure class="image" data-testid="image-view" aria-label="Image {name}">
  {#if failed}
    <p class="note">This image cannot be shown.</p>
  {:else}
    <img {src} alt={name} {onload} onerror={() => (failed = true)} />
  {/if}
  {#if size}<figcaption>{name}, {size.width} × {size.height} pixels</figcaption>{/if}
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
    background: repeating-conic-gradient(#1f1f25 0% 25%, #19191e 0% 50%) 0 0 / 16px 16px;
  }
  img {
    max-width: 100%;
    max-height: calc(100% - 24px);
    object-fit: contain;
  }
  figcaption,
  .note {
    color: #b4b4c0;
    font-size: 12px;
  }
</style>
