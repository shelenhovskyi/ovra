<script lang="ts">
// Placeholder. Task 1.C: fuzzy command list; closes by setting workspace.paletteOpen = false
import { useOvra } from "../shell/context";

const { workspace } = useOvra();
</script>

<div class="placeholder" data-panel="Command palette">
  <p>Command palette</p>
  {#if workspace.openPath}<p class="path">{workspace.openPath}</p>{/if}
</div>

<style>
  .placeholder {
    padding: 12px;
    color: #6f6f7a;
  }
</style>
