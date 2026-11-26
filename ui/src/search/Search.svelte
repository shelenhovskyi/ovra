<script lang="ts">
// Sidebar search view for the open project, and the "Search in files" and
// "Replace in files" commands.
import { tick, untrack } from "svelte";
import { focusMain, tabs } from "../editor/open-files.svelte";
import { leaveFocus } from "../focus/mode.svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { handleFindInFolder } from "./find-in";
import SearchPanel from "./SearchPanel.svelte";

const { transport, workspace } = useOvra();

let panel = $state<SearchPanel | null>(null);

async function show(): Promise<void> {
  leaveFocus();
  workspace.sidebar = "search";
  await tick();
  panel?.focus();
}

async function showReplace(): Promise<void> {
  leaveFocus();
  workspace.sidebar = "search";
  await tick();
  await panel?.focusReplace();
}

async function findIn(include: string): Promise<void> {
  leaveFocus();
  workspace.sidebar = "search";
  await tick();
  panel?.limitTo(include);
}

$effect(() => handleFindInFolder(findIn));
$effect(() => registerCommand({ id: "search.show", title: "Search in files", keys: "Mod+Shift+F", run: show }));
$effect(() =>
  registerCommand({ id: "search.replace", title: "Replace in files", keys: "Mod+Shift+H", run: showReplace }),
);

/** The open editor buffer of `path`, if the editor shows this project. */
function buffer(path: string) {
  const current = tabs.current;
  return current && current.project === workspace.project?.id ? current.buffers.get(path) : undefined;
}

// Choosing the Search view (its sidebar tab) puts the cursor in the box, so
// typing searches; the view shown at start takes no focus.
let shown = untrack(() => workspace.sidebar);
$effect(() => {
  const now = workspace.sidebar;
  if (now === "search" && shown !== "search") void tick().then(() => panel?.focus());
  shown = now;
});
</script>

{#if workspace.project}
  {#key workspace.project.id}
    <SearchPanel
      bind:this={panel}
      {transport}
      project={workspace.project.id}
      onopen={(path, at) => workspace.openFile(path, "edit", at)}
      onleave={() => void focusMain()}
      {buffer}
    />
  {/key}
{/if}
