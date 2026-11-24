<script lang="ts">
// Sidebar: the start screen until a project is open, then its file tree.
import { useOvra } from "../shell/context";
import FileTree from "./FileTree.svelte";
import ProjectList from "./ProjectList.svelte";

const { transport, workspace } = useOvra();
</script>

<h2 class="visually-hidden">File tree</h2>
{#if workspace.project}
  {#key workspace.project.id}
    <FileTree
      {transport}
      project={workspace.project.id}
      openPath={workspace.openPath}
      onopen={(path) => workspace.openFile(path, workspace.mode)}
    />
  {/key}
{:else}
  <ProjectList {transport} onopen={(project) => (workspace.project = project)} />
{/if}

<style>
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
