<script lang="ts">
// Sidebar: the start screen until a project is open, then its file tree.
// Mod+O (the system's folder dialog) opens a folder from both; dropping a
// folder onto the window opens it from the start screen only, so a drop on
// the editor or a terminal never switches the project.
import { untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import FileTree from "./FileTree.svelte";
import { FolderOpener } from "./folder-opener.svelte";
import ProjectList from "./ProjectList.svelte";

const { transport, workspace } = useOvra();
const opener = new FolderOpener(transport, (project) => {
  workspace.openPath = null;
  workspace.mode = "view";
  workspace.project = project;
});

$effect(() => {
  if (!opener.canPick) return;
  return registerCommand({ id: "tree.open_folder", title: "Open folder…", keys: "Mod+O", run: () => opener.pick() });
});

$effect(() =>
  transport.onDrop?.((paths) => {
    if (!workspace.project) void opener.drop(paths);
  }),
);

// A failed Mod+O with a project on screen shows here, in the files view.
$effect(() => {
  if (opener.error && workspace.project) untrack(() => (workspace.sidebar = "files"));
});

// An error about another folder does not follow onto the start screen.
$effect(() => {
  if (!workspace.project) untrack(() => (opener.error = null));
});
</script>

<h2 class="visually-hidden">File tree</h2>
{#if workspace.project}
  {#if opener.error}
    <p class="error" role="alert">
      {opener.error}
      <button type="button" onclick={() => (opener.error = null)}>Dismiss</button>
    </p>
  {/if}
  {#key workspace.project.id}
    <FileTree
      {transport}
      project={workspace.project.id}
      openPath={workspace.openPath}
      onopen={(path) => workspace.openFile(path, workspace.mode)}
      onreveal={() => (workspace.sidebar = "files")}
    />
  {/key}
{:else}
  <ProjectList {transport} {opener} />
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
  .error {
    margin: 0;
    padding: 6px 12px;
    color: #ff6b6b;
  }
  .error button {
    margin-left: 6px;
    border: 1px solid #2a2a31;
    background: #22222a;
    color: inherit;
    font: inherit;
  }
</style>
