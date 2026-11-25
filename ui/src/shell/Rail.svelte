<script lang="ts">
// The icon rail beside the rail's panel: what that panel shows (Files,
// Search, Git), and Terminal and Settings at the bottom. Pressing the shown view
// again folds the panel; another view unfolds it. The shell moves the rail
// to the right edge when the panel goes there, and hides it on the start
// screen. Reviews and More are in the title bar.
import { listCommands } from "../keys/commands";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import type { IconName } from "./icons";
import type { Layout } from "./layout.svelte";
import { hush, warmTips } from "./tips";
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

<!-- The press only hides a tooltip; the buttons inside do the work. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<nav class="rail" aria-label="Views" onpointerdown={hush} {@attach warmTips}>
  {#if workspace.project}
    {#each VIEWS as view (view.id)}
      <button
        type="button"
        class="rail-button tip"
        data-tip={view.name}
        aria-label={view.name}
        aria-pressed={workspace.sidebar === view.id && !layout.side.collapsed}
        onclick={() => choose(view.id)}><Icon name={view.icon} /></button
      >
    {/each}
    <span class="grow"></span>
    <button type="button" class="rail-button tip" data-tip="Terminal" aria-label="Terminal" onclick={() => run("terminal.toggle")}
      ><Icon name="terminal" /></button
    >
    <button type="button" class="rail-button tip" data-tip="Settings" aria-label="Settings" onclick={() => run("palette.settings")}
      ><Icon name="settings" /></button
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
  .rail :global(.rail-button) {
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
  .rail :global(.rail-button:hover) {
    color: var(--text);
    background: var(--s3);
  }
  .rail :global(.rail-button[aria-pressed="true"]) {
    color: var(--text);
    background: var(--s2);
  }
  /* Tooltips: the button's name beside it, after a short hover or on keyboard focus; a press hides it. */
  .rail :global(.tip::after) {
    content: attr(data-tip);
    position: absolute;
    left: calc(100% + 8px);
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
  /* On the right edge the tooltip opens to the left. */
  :global(.rail-right) .rail :global(.tip::after) {
    left: auto;
    right: calc(100% + 8px);
  }
  .rail :global(.tip:hover::after) {
    opacity: 1;
    transition-delay: 0.6s;
  }
  /* Warm: another tooltip was just shown, so this one shows at once. */
  .rail:global([data-warm]) :global(.tip:hover::after) {
    transition-delay: 0s;
  }
  .rail :global(.tip:focus-visible::after) {
    opacity: 1;
  }
  .rail :global(.tip[data-hush]::after) {
    opacity: 0;
    transition: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .rail :global(.tip::after) {
      transition: none;
    }
  }
  @media (forced-colors: active) {
    .rail :global(.rail-button[aria-pressed="true"]) {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
