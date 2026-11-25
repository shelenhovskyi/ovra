<script lang="ts">
// Sidebar: the start screen until a project is open, then its file tree.
// Mod+O (the system's folder dialog) opens a folder from both; dropping a
// folder onto the window opens it from the start screen only, so a drop on
// the editor or a terminal never switches the project.
import { tick, untrack } from "svelte";
import { modeOf, requestClose, tabs } from "../editor/open-files.svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import FileTree from "./FileTree.svelte";
import { FolderOpener } from "./folder-opener.svelte";
import { isWithin, movedPath } from "./ops";
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

// Back to the files from Search or Git, as Mod+Shift+F and Mod+Shift+G go there.
$effect(() => {
  if (!workspace.project) return;
  return registerCommand({
    id: "tree.focus",
    title: "Show files",
    keys: "Mod+Shift+E",
    run: async () => {
      workspace.sidebar = "files";
      await tick();
      document.querySelector<HTMLElement>('[role="tree"]')?.focus();
    },
  });
});

$effect(() =>
  transport.onDrop?.((paths) => {
    if (!workspace.project) void opener.drop(paths);
  }),
);

// TODO(tabs): until the tabs can follow a path themselves (`renamePath`,
// `forgetPaths` in open-files), a moved file's tab is closed and reopened
// at its new path and a trashed file's tab closes. Tabs with unsaved text
// stay as they are, so nothing typed is lost.
function moved(from: string, to: string): void {
  const current = tabs.current;
  if (!current) return;
  for (const path of [...current.files]) {
    const next = movedPath(path, from, to);
    if (!next || current.buffers.isDirty(path)) continue;
    const shown = path === workspace.openPath;
    const mode = workspace.mode;
    requestClose(workspace, path);
    if (shown) workspace.openFile(next, mode);
    else current.keep(next);
  }
}

function trashed(paths: string[]): void {
  const current = tabs.current;
  for (const path of [...(current?.files ?? [])]) {
    if (paths.some((p) => isWithin(path, p)) && !current?.buffers.isDirty(path)) requestClose(workspace, path);
  }
}

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
      root={workspace.project.root}
      onopen={(path, mode) => workspace.openFile(path, mode ?? modeOf(path))}
      onreveal={() => (workspace.sidebar = "files")}
      onmoved={moved}
      ontrashed={trashed}
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
    color: var(--danger);
  }
  .error button {
    margin-left: 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--s3);
    color: var(--text);
    font: inherit;
  }
</style>
