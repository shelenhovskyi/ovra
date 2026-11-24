<script lang="ts">
// Placeholder. Task 1.E: markdown, images and diffs for workspace.openPath
import { useOvra } from "../shell/context";

const { workspace } = useOvra();
</script>

<div class="placeholder" data-panel="Viewer">
  <p>Viewer</p>
  {#if workspace.openPath}<p class="path">{workspace.openPath}</p>{/if}
</div>

<style>
  .placeholder {
    padding: 12px;
    color: #6f6f7a;
  }
</style>
