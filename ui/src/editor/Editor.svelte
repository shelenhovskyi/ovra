<script lang="ts">
// Placeholder. Task 1.F: CodeMirror 6 with tree-sitter highlighting for workspace.openPath
import { useOvra } from "../shell/context";

const { workspace } = useOvra();
</script>

<div class="placeholder" data-panel="Editor">
  <p>Editor</p>
  {#if workspace.openPath}<p class="path">{workspace.openPath}</p>{/if}
</div>

<style>
  .placeholder {
    padding: 12px;
    color: #6f6f7a;
  }
</style>
