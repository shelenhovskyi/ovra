<script lang="ts">
// Placeholder. Task 1.D: virtualized tree with git status; calls fs.list_dir and workspace.openFile()
import { useOvra } from "../shell/context";

const { workspace } = useOvra();
</script>

<div class="placeholder" data-panel="File tree">
  <p>File tree</p>
  {#if workspace.openPath}<p class="path">{workspace.openPath}</p>{/if}
</div>

<style>
  .placeholder {
    padding: 12px;
    color: #6f6f7a;
  }
</style>
