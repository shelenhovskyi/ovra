<script lang="ts">
// One editor group of the main area (docs/specs/split-editor.md): its tab
// strip or its file. Panels inside see the workspace as this group's
// (`groupView`). Focus or a click inside focuses the group, and a tab
// dragged from the other group can be dropped here.
import Editor from "../editor/Editor.svelte";
import { dragging, moveTab } from "../editor/open-files.svelte";
import Tabs from "../editor/Tabs.svelte";
import Viewer from "../viewers/Viewer.svelte";
import { provideOvra, useOvra } from "./context";
import { type GroupIndex, groupView } from "./workspace.svelte";

let { group, part }: { group: GroupIndex; part: "tabs" | "file" } = $props();

const { transport, workspace } = useOvra();
// svelte-ignore state_referenced_locally
const own = groupView(workspace, group);
provideOvra({ transport, workspace: own });

/** A tab from the other group is dragged over this one. */
let over = $state(false);

function ondragover(event: DragEvent): void {
  const tab = dragging.tab;
  if (!tab || tab.group === group) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  over = true;
}

function ondrop(event: DragEvent): void {
  const tab = dragging.tab;
  over = false;
  if (!tab || tab.group === group) return;
  event.preventDefault();
  dragging.tab = null;
  moveTab(workspace, tab.path, group);
}

function focus(): void {
  workspace.focusGroup(group);
}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group {part}"
  class:over
  data-group={group}
  data-focused={own.focused || undefined}
  {ondragover}
  ondragleave={() => (over = false)}
  {ondrop}
  onfocusin={focus}
  onpointerdown={focus}
>
  {#if part === "tabs"}
    <Tabs {group} />
  {:else if own.mode === "edit"}
    <Editor />
  {:else}
    <Viewer />
  {/if}
</div>

<style>
  .group {
    min-width: 0;
    min-height: 0;
  }
  .file {
    height: 100%;
  }
  .over {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
  }
</style>
