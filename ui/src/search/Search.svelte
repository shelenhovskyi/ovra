<script lang="ts">
// Sidebar search view for the open project, and the "Search in files" command.
import { tick, untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import SearchPanel from "./SearchPanel.svelte";

const { transport, workspace } = useOvra();

let panel = $state<SearchPanel | null>(null);

async function show(): Promise<void> {
  workspace.sidebar = "search";
  await tick();
  panel?.focus();
}

$effect(() => registerCommand({ id: "search.show", title: "Search in files", keys: "Mod+Shift+F", run: show }));

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
    />
  {/key}
{/if}
