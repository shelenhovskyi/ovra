<script lang="ts">
// The icon rail on the window's left edge: what the side panel shows (Files,
// Search, Git) and the terminal. Pressing the shown view again folds the
// panel; another view unfolds it.
import { listCommands } from "../keys/commands";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import type { IconName } from "./icons";
import type { Layout } from "./layout.svelte";
import type { SidebarView } from "./workspace.svelte";

let { layout }: { layout: Layout } = $props();

const { workspace } = useOvra();

const VIEWS: { id: SidebarView; name: string; icon: IconName }[] = [
  { id: "files", name: "Files", icon: "files" },
  { id: "search", name: "Search", icon: "search" },
  { id: "git", name: "Git", icon: "branch" },
];

function choose(view: SidebarView): void {
  const shown = workspace.sidebar === view && !layout.side.collapsed;
  workspace.sidebar = view;
  layout.setCollapsed("side", shown);
}

function run(id: string): void {
  void listCommands()
    .find((c) => c.id === id)
    ?.run();
}
</script>

<nav class="rail" aria-label="Views">
  {#if workspace.project}
    {#each VIEWS as view (view.id)}
      <button
        type="button"
        class="tip"
        data-tip={view.name}
        aria-label={view.name}
        aria-pressed={workspace.sidebar === view.id && !layout.side.collapsed}
        onclick={() => choose(view.id)}><Icon name={view.icon} /></button
      >
    {/each}
    <span class="grow"></span>
    <button type="button" class="tip" data-tip="Terminal" aria-label="Terminal" onclick={() => run("terminal.toggle")}
      ><Icon name="terminal" /></button
    >
  {/if}
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 6px 0 10px;
  }
  .grow {
    flex: 1;
  }
  button {
    position: relative;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    padding: 0;
    color: var(--text-3);
    background: none;
    border: 0;
    border-radius: 9px;
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s3);
  }
  button[aria-pressed="true"] {
    color: var(--text);
    background: var(--s2);
  }
  /* Tooltips: the button's name to its right, on hover or keyboard focus. */
  .tip::after {
    content: attr(data-tip);
    position: absolute;
    left: calc(100% + 10px);
    top: 50%;
    z-index: 50;
    padding: 4px 9px;
    font-size: 13px;
    white-space: nowrap;
    color: var(--text);
    background: var(--s4);
    border-radius: 6px;
    box-shadow: var(--card-shadow);
    transform: translateY(-50%);
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.12s;
  }
  .tip:hover::after,
  .tip:focus-visible::after {
    opacity: 1;
  }
  @media (prefers-reduced-motion: reduce) {
    .tip::after {
      transition: none;
    }
  }
  @media (forced-colors: active) {
    button[aria-pressed="true"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
