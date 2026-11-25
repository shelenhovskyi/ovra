<script lang="ts">
// The window's top row: project, branch, a search field that opens the
// palette, Reviews and More. In the desktop app on macOS it is the title bar
// itself (`data-chrome="overlay"` on the root, set by the app): the traffic
// lights sit on its left and its empty space drags the window.
import AuditLog from "../audit/AuditLog.svelte";
import BrowserButton from "../browser/BrowserButton.svelte";
import Inbox from "../inbox/Inbox.svelte";
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import NetworkPanel from "../network/NetworkPanel.svelte";
import PolicyPanel from "../policy/PolicyPanel.svelte";
import ServersPanel from "../servers/ServersPanel.svelte";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import MoreMenu from "./MoreMenu.svelte";
import ProjectSwitcher from "./ProjectSwitcher.svelte";

// Mod+K. The palette it opens lists files (projects on the start screen), not
// commands, so the field says "Go to file" rather than "Search or run".
const SEARCH = "palette.search";

const { transport, workspace } = useOvra();

let branch = $state<string | null>(null);
/** The palette's live shortcut, read once the commands are registered. */
let keys = $state<string | undefined>();

$effect(() => {
  const command = listCommands().find((c) => c.id === SEARCH);
  keys = command?.keys ? liveKeys(command.keys, inBrowserTab()) : undefined;
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

function search(): void {
  void listCommands()
    .find((c) => c.id === SEARCH)
    ?.run();
}
</script>

<header class="title" data-tauri-drag-region>
  <div class="start" data-tauri-drag-region>
    <h1 class="mark">
      <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <circle class="ring" cx="12" cy="12" r="7.5" />
        <circle class="dot" cx="12" cy="12" r="2.2" />
      </svg>
      <span class="visually-hidden">Ovra</span>
    </h1>
    {#if workspace.project}
      <ProjectSwitcher project={workspace.project} />
      {#if branch}
        <span class="branch" title="Branch"><Icon name="branch" small /><span class="name">{branch}</span></span>
      {/if}
    {:else}
      <span class="none" data-testid="project-name">No project open</span>
    {/if}
  </div>
  <button type="button" class="search" data-testid="search-field" aria-keyshortcuts={keys} onclick={search}>
    <Icon name="search" small />
    <span class="label">{workspace.project ? "Go to file" : "Open a project"}</span>
    {#if keys}<kbd aria-hidden="true">{formatShortcut(keys, isApple())}</kbd>{/if}
  </button>
  <span class="actions" data-tauri-drag-region>
    <Inbox />
    <MoreMenu />
  </span>
  <!-- Opened from the "More" menu, the palette or their shortcuts. -->
  <AuditLog trigger={false} />
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
    padding: 0 10px 0 16px;
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
    margin: 0;
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
  .none {
    color: var(--text-2);
  }
  .branch {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text-2);
  }
  .branch .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 13px var(--mono);
  }
  .start,
  .actions {
    display: flex;
    align-items: center;
    align-self: stretch;
    min-width: 0;
  }
  .start {
    gap: 10px;
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
  .actions {
    justify-content: flex-end;
    gap: 4px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    .search {
      border: 1px solid ButtonText;
    }
  }
</style>
