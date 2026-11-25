<script lang="ts">
// The window's top row, in three groups (docs/design/components.md): the
// sidebar toggle, project and branch; a search field that opens the palette;
// Reviews, the Agents toggle and More. The start screen keeps only the Ovra
// mark and More. In the desktop app on macOS it is the title bar itself
// (`data-chrome="overlay"` on the root, set by the app): the traffic lights
// sit on its left and its empty space drags the window.
import BrowserButton from "../browser/BrowserButton.svelte";
import Inbox from "../inbox/Inbox.svelte";
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import NetworkPanel from "../network/NetworkPanel.svelte";
import PolicyPanel from "../policy/PolicyPanel.svelte";
import ServersPanel from "../servers/ServersPanel.svelte";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import type { Layout, Panel } from "./layout.svelte";
import MoreMenu from "./MoreMenu.svelte";
import ProjectSwitcher from "./ProjectSwitcher.svelte";
import { hush, warmTips } from "./tips";

let {
  layout,
  shown,
  waiting,
  toggle,
  showApprovals,
}: {
  layout: Layout;
  /** Whether each side panel is on screen. */
  shown: Record<Panel, boolean>;
  /** Requests that wait for a person while Agents is hidden. */
  waiting: number;
  toggle: (panel: Panel) => void;
  showApprovals: () => void;
} = $props();

// Mod+K: "Search or run", files and commands.
const SEARCH = "palette.search";
const TOGGLES: Record<Panel, { name: string; command: string }> = {
  side: { name: "Sidebar", command: "layout.toggle_side" },
  agents: { name: "Agents", command: "layout.toggle_agents" },
};

const { transport, workspace } = useOvra();

let branch = $state<string | null>(null);
/** The live shortcut of each command, read once the panels registered theirs. */
let keys = $state<Record<string, string>>({});

$effect(() => {
  void workspace.project;
  const live: Record<string, string> = {};
  for (const command of listCommands()) if (command.keys) live[command.id] = liveKeys(command.keys, inBrowserTab());
  keys = live;
});

// The branch of the open project, again whenever git says something changed.
$effect(() => {
  const project = workspace.project?.id;
  branch = null;
  if (!project) return;
  let latest = 0;
  const load = () => {
    const mine = ++latest;
    transport
      .call({ method: "git.status", params: { project } })
      .then((reply) => {
        if (mine === latest) branch = reply.type === "git.status" ? reply.data.branch : null;
      })
      .catch(() => {
        // Not a git repository, or the core is gone: no branch to show.
        if (mine === latest) branch = null;
      });
  };
  load();
  const off = transport.onEvent((event) => {
    if (event.type === "fs.git_changed" && event.data.project === project) load();
  });
  return () => {
    latest = -1;
    off();
  };
});

function run(id: string): void {
  void listCommands()
    .find((c) => c.id === id)
    ?.run();
}

/** The branch menu is the Git panel's: show the panel, then the menu. */
function switchBranch(): void {
  if (!shown.side) toggle("side");
  run("git.switch_branch");
}

function tip(panel: Panel): string {
  const { name, command } = TOGGLES[panel];
  const shortcut = keys[command] ? ` ${formatShortcut(keys[command], isApple())}` : "";
  return `${shown[panel] ? "Hide" : "Show"} ${name}${shortcut}`;
}
</script>

{#snippet panelToggle(panel: Panel)}
  <button
    type="button"
    class="tool tip"
    data-tip={tip(panel)}
    aria-label={TOGGLES[panel].name}
    aria-pressed={shown[panel]}
    onclick={() => toggle(panel)}><Icon name={layout.left === panel ? "panel-left" : "panel-right"} /></button
  >
{/snippet}

<!-- The press only hides a tooltip; the buttons inside do the work. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="title" data-tauri-drag-region onpointerdown={hush} {@attach warmTips}>
  <div class="lead" data-tauri-drag-region>
    <h1 class="mark" class:visually-hidden={!!workspace.project}>
      <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <circle class="ring" cx="12" cy="12" r="7.5" />
        <circle class="dot" cx="12" cy="12" r="2.2" />
      </svg>
      <span class="visually-hidden">Ovra</span>
    </h1>
    {#if workspace.project}
      {@render panelToggle("side")}
      <ProjectSwitcher project={workspace.project} />
      {#if branch}
        <button type="button" class="branch" title="Switch branch" onclick={switchBranch}
          ><Icon name="branch" small /><span class="name">{branch}</span><span class="visually-hidden">
            (switch branch)</span
          ></button
        >
      {/if}
    {/if}
  </div>
  {#if workspace.project}
    <button type="button" class="search" data-testid="search-field" aria-keyshortcuts={keys[SEARCH]} onclick={() => run(SEARCH)}>
      <Icon name="search" small />
      <span class="label">Search or run</span>
      {#if keys[SEARCH]}<kbd aria-hidden="true">{formatShortcut(keys[SEARCH], isApple())}</kbd>{/if}
    </button>
  {:else}
    <span data-tauri-drag-region></span>
  {/if}
  <div class="trail" data-tauri-drag-region>
    {#if waiting > 0}
      <button
        type="button"
        class="waiting"
        data-testid="approvals-waiting"
        aria-label={`${waiting} waiting for approval, show Agents`}
        onclick={showApprovals}>{waiting} waiting</button
      >
    {/if}
    <!-- Mounted with or without a project: its shortcut and dialog work everywhere. -->
    <span class="reviews" hidden={!workspace.project}><Inbox /></span>
    {#if workspace.project}{@render panelToggle("agents")}{/if}
    <MoreMenu />
  </div>
  <!-- Opened from the "More" menu, the palette or their shortcuts. -->
  <PolicyPanel trigger={false} />
  <NetworkPanel trigger={false} />
  <ServersPanel trigger={false} />
  <BrowserButton trigger={false} />
</header>

<style>
  /* Three columns keep the search field in the middle of the window. */
  .title {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 360px) minmax(0, 1fr);
    align-items: center;
    gap: 16px;
    min-width: 0;
    height: 100%;
    padding: 0 8px 0 12px;
    user-select: none;
  }
  /* The desktop app's overlay title bar: room for the window's traffic lights.
     Their position is in app/tauri.conf.json: tao centres the buttons in a
     box "button + y" tall, so y = this bar's height (36 px) - 14 = 22. */
  :global([data-chrome="overlay"]) .title {
    padding-left: 80px;
  }
  .mark {
    display: flex;
    margin: 0 0 0 4px;
  }
  .mark svg {
    width: 18px;
    height: 18px;
  }
  .ring {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2.4;
  }
  .dot {
    fill: var(--text);
  }
  .lead,
  .trail {
    display: flex;
    align-items: center;
    align-self: stretch;
    gap: 4px;
    min-width: 0;
  }
  .trail {
    justify-content: flex-end;
  }
  .reviews[hidden] {
    display: none;
  }
  /* Title-bar items, Reviews' and More's included (they carry the rail's class):
     borderless 28 px squares. */
  .tool,
  .trail :global(.rail-button) {
    position: relative;
    display: grid;
    flex: none;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: 7px;
    cursor: pointer;
  }
  .tool:hover,
  .trail :global(.rail-button:hover),
  .trail :global(.rail-button[aria-expanded="true"]) {
    color: var(--text);
    background: var(--s3);
  }
  .tool[aria-pressed="false"] {
    color: var(--text-3);
  }
  .trail :global(.rail-badge) {
    position: absolute;
    top: -3px;
    right: -5px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font: 600 12px / 16px var(--font);
    text-align: center;
    color: var(--accent-ink);
    background: var(--accent);
    border-radius: 8px;
  }
  .waiting {
    height: 24px;
    padding: 0 10px;
    font: 500 13px var(--font);
    color: var(--warn);
    background: var(--warn-soft);
    border: 0;
    border-radius: 12px;
    cursor: pointer;
  }
  .waiting:hover {
    filter: brightness(1.1);
  }
  /* Tooltips below the button, after a short hover or on keyboard focus; a press hides it. */
  .title :global(.tip::after) {
    content: attr(data-tip);
    position: absolute;
    top: calc(100% + 6px);
    z-index: 50;
    padding: 4px 9px;
    font: 400 13px var(--font);
    white-space: nowrap;
    color: var(--text);
    background: var(--s4);
    border-radius: 6px;
    box-shadow: var(--card-shadow);
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.12s;
  }
  .lead :global(.tip::after) {
    left: 0;
  }
  .trail :global(.tip::after) {
    right: 0;
  }
  .title :global(.tip:hover::after) {
    opacity: 1;
    transition-delay: 0.6s;
  }
  .title:global([data-warm]) :global(.tip:hover::after) {
    transition-delay: 0s;
  }
  .title :global(.tip:focus-visible::after) {
    opacity: 1;
  }
  .title :global(.tip[data-hush]::after) {
    opacity: 0;
    transition: none;
  }
  .branch {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: 7px;
    cursor: pointer;
  }
  .branch:hover {
    color: var(--text);
    background: var(--s3);
  }
  .branch .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 13px var(--mono);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 30px;
    padding: 0 6px 0 11px;
    font: inherit;
    color: var(--text-2);
    background: var(--s1);
    border: 0;
    border-radius: 8px;
    cursor: pointer;
  }
  .search:hover {
    color: var(--text);
    background: var(--s3);
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  kbd {
    padding: 3px 6px;
    font: 500 12px / 1 var(--font);
    color: var(--text-2);
    background: var(--s3);
    border-radius: 5px;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (prefers-reduced-motion: reduce) {
    .title :global(.tip::after) {
      transition: none;
    }
  }
  @media (forced-colors: active) {
    .search {
      border: 1px solid ButtonText;
    }
    .tool[aria-pressed="true"] {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
