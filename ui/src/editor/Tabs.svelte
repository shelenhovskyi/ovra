<script lang="ts">
// Tabs for the open files of the project, above the main area.
import { useOvra } from "../shell/context";
import { show, tabs } from "./open-files.svelte";
import { hasUnsavedChanges } from "./session.svelte";

const { workspace } = useOvra();

function name(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function close(path: string): void {
  const current = tabs.current;
  if (!current) return;
  const wasActive = current.active === path;
  const next = current.close(path);
  if (wasActive) show(workspace, next);
}
</script>

{#if tabs.current && tabs.current.files.length > 0}
  {@const current = tabs.current}
  <div class="tabs" role="tablist" aria-label="Open files" data-testid="tabs">
    {#each current.files as path (path)}
      {@const active = path === current.active}
      {@const dirty = hasUnsavedChanges(current.project, path)}
      <div class="tab" class:active title={path}>
        <button type="button" role="tab" aria-selected={active} onclick={() => show(workspace, path)}>
          {name(path)}
        </button>
        <button
          type="button"
          class="close"
          class:dirty
          aria-label={dirty ? `Close ${name(path)} (unsaved changes)` : `Close ${name(path)}`}
          onclick={() => close(path)}
        ></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .tabs {
    display: flex;
    overflow-x: auto;
    border-bottom: 1px solid #2a2a31;
    background: #121215;
    scrollbar-width: thin;
  }
  .tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    border-right: 1px solid #2a2a31;
    color: #9a9aa6;
  }
  .tab.active {
    background: #16161a;
    color: #e6e6e6;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: pointer;
  }
  [role="tab"] {
    padding: 6px 4px 6px 12px;
  }
  .close {
    width: 20px;
    height: 20px;
    margin-right: 6px;
    border-radius: 3px;
  }
  .close::before {
    content: "×";
  }
  .close.dirty::before {
    content: "●";
    font-size: 10px;
  }
  .close:hover {
    background: #2a2a31;
  }
  .close.dirty:hover::before {
    content: "×";
    font-size: inherit;
  }
</style>
