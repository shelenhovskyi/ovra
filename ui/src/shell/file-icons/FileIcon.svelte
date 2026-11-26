<script lang="ts">
// A 16 px file or folder icon by name (Material Icon Theme, see icons.svelte.ts).
// Decorative: the name next to it says the same. Empty until the table loads.
import { iconUrl, loadIcons } from "./icons.svelte";

let {
  name,
  kind = "file",
  open = false,
}: { name: string; kind?: "file" | "dir" | "symlink"; open?: boolean } = $props();

void loadIcons();
const url = $derived(iconUrl(name, kind, open));
</script>

{#if url}<img class="file-icon" src={url} alt="" width="16" height="16" draggable="false" />{:else}<span
    class="file-icon"
    aria-hidden="true"
  ></span>{/if}

<style>
  .file-icon {
    flex: none;
    width: var(--icon-sm);
    height: var(--icon-sm);
  }
</style>
