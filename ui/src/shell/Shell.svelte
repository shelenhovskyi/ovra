<script lang="ts">
import { untrack } from "svelte";
import AgentsPanel from "../agents/AgentsPanel.svelte";
import Approvals from "../approvals/Approvals.svelte";
import AuditLog from "../audit/AuditLog.svelte";
import BrowserButton from "../browser/BrowserButton.svelte";
import BrowserPane from "../browser/BrowserPane.svelte";
import Editor from "../editor/Editor.svelte";
import { persistOpenFiles } from "../editor/open-files.svelte";
import Tabs from "../editor/Tabs.svelte";
import Git from "../git/Git.svelte";
import GitDiff from "../git/GitDiff.svelte";
import Inbox from "../inbox/Inbox.svelte";
import Keybindings from "../keys/Keybindings.svelte";
import type { CoreInfo } from "../lib/protocol";
import type { ConnectionStatus, Transport } from "../lib/transport";
import LanguageServers from "../lsp/LanguageServers.svelte";
import NetworkPanel from "../network/NetworkPanel.svelte";
import Notifications from "../notify/Notifications.svelte";
import Palette from "../palette/Palette.svelte";
import PolicyPanel from "../policy/PolicyPanel.svelte";
import Search from "../search/Search.svelte";
import ServersPanel from "../servers/ServersPanel.svelte";
import Terminals from "../terminal/Terminals.svelte";
import Tree from "../tree/Tree.svelte";
import Viewer from "../viewers/Viewer.svelte";
import { provideOvra } from "./context";
import { rememberProject, restoreProject } from "./last-project";
import MoreMenu from "./MoreMenu.svelte";
import ProjectSwitcher from "./ProjectSwitcher.svelte";
import { Workspace } from "./workspace.svelte";

let { transport }: { transport: Transport } = $props();

const workspace = new Workspace();
// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });

let info = $state<CoreInfo | null>(null);
let error = $state<string | null>(null);
let connection = $state<ConnectionStatus>("connecting");
let wasOpen = $state(false);
let everClosed = $state(false);
let request = 0;
/** The git diff the main area shows instead of the open file, if it is of this project. */
const gitDiff = $derived(workspace.gitDiff?.project === workspace.project?.id ? workspace.gitDiff : null);

$effect(() => persistOpenFiles(transport, workspace));

// The window opens the project it had before a reload; until that is decided, nothing is forgotten.
let restoring = true;
$effect(() =>
  untrack(() => {
    void restoreProject(transport, workspace).finally(() => {
      restoring = false;
    });
  }),
);
$effect(() => {
  const id = workspace.project?.id ?? null;
  untrack(() => {
    if (id || !restoring) rememberProject(id);
  });
});

/** Asks the core who it is; only the latest request may update the status. */
function requestInfo() {
  const mine = ++request;
  transport
    .call({ method: "info.get" })
    .then((response) => {
      if (mine !== request) return;
      info = response.data;
      error = null;
    })
    .catch((err: unknown) => {
      if (mine !== request) return;
      error = err instanceof Error ? err.message : String(err);
    });
}

$effect(() =>
  untrack(() => {
    if (!transport.onStatus) {
      connection = "open";
      requestInfo();
      return;
    }
    return transport.onStatus((status) => {
      connection = status;
      if (status !== "open") {
        info = null;
        if (status === "closed") everClosed = true;
        return;
      }
      wasOpen = true;
      requestInfo();
    });
  }),
);

const status = $derived.by((): { text: string; ok: boolean } => {
  if (connection === "open") {
    if (info) return { text: "Connected", ok: true };
    if (error) return { text: `Core unavailable: ${error}`, ok: false };
    return { text: "Connecting…", ok: true };
  }
  if (connection === "refused") return { text: "Core unavailable: it no longer accepts this address", ok: false };
  if (wasOpen) return { text: "Connection to core lost, reconnecting…", ok: false };
  if (everClosed || error) return { text: `Core unavailable: ${error ?? "could not connect, retrying…"}`, ok: false };
  return { text: "Connecting…", ok: true };
});
</script>

<Keybindings />
<div class="shell">
  <header>
    <h1>Ovra</h1>
    {#if workspace.project}
      <ProjectSwitcher project={workspace.project} />
    {:else}
      <span class="project muted" data-testid="project-name">No project open</span>
    {/if}
    <Inbox />
    <MoreMenu />
    <!-- Opened from the "More" menu, the palette or their shortcuts. -->
    <AuditLog trigger={false} />
    <PolicyPanel trigger={false} />
    <NetworkPanel trigger={false} />
    <ServersPanel trigger={false} />
    <BrowserButton trigger={false} />
  </header>
  <aside data-testid="sidebar">
    {#if workspace.project}
      <div class="sidebar-tabs" role="group" aria-label="Sidebar view">
        <button
          type="button"
          aria-pressed={workspace.sidebar === "files"}
          onclick={() => (workspace.sidebar = "files")}>Files</button
        >
        <button
          type="button"
          aria-pressed={workspace.sidebar === "search"}
          onclick={() => (workspace.sidebar = "search")}>Search</button
        >
        <button
          type="button"
          aria-pressed={workspace.sidebar === "git"}
          onclick={() => (workspace.sidebar = "git")}>Git</button
        >
      </div>
    {/if}
    <div class="sidebar-view" hidden={!!workspace.project && workspace.sidebar !== "files"}><Tree /></div>
    <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "search"}><Search /></div>
    <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "git"}><Git /></div>
  </aside>
  <main data-testid="main">
    <Tabs />
    <!-- The selected tab: a git diff (a tab of its own) or the open file, which stays mounted under the diff. -->
    <div
      class="view"
      id="ovra-tabpanel"
      role="tabpanel"
      tabindex="-1"
      aria-label={gitDiff ? `Diff of ${gitDiff.path}` : (workspace.openPath ?? "No file open")}
    >
      <!-- An each block keeps the closing diff's target: an if block would hand it null while it unmounts. -->
      {#each gitDiff ? [gitDiff] : [] as target (target)}
        <GitDiff {target} />
      {/each}
      <div class="file" hidden={!!gitDiff}>
        {#if workspace.mode === "edit"}
          <Editor />
        {:else}
          <Viewer />
        {/if}
      </div>
    </div>
    <BrowserPane />
    <Terminals />
  </main>
  <aside class="agents" data-testid="agents" aria-label="Agents and approvals">
    <Approvals />
    <AgentsPanel />
  </aside>
  <footer>
    <span
      data-testid="status"
      role="status"
      class:error={!status.ok}
      title={info ? `${info.name} ${info.version} (${info.mode})` : undefined}>{status.text}</span
    >
    <LanguageServers />
  </footer>
  <Notifications />
  {#if connection === "refused"}
    <section class="refused" role="alert" aria-labelledby="refused-title">
      <h2 id="refused-title">Ovra was restarted</h2>
      <p>This page uses the address of an earlier run, and the new run does not accept it.</p>
      <p>
        Open the new address instead. Ovra prints it when it starts, on a line that begins with
        <code>OVRA_URL=</code> (for example in the terminal running <code>just dev-headless</code>).
      </p>
    </section>
  {/if}
  {#if workspace.paletteOpen}
    <Palette />
  {/if}
</div>

<style>
  .shell {
    display: grid;
    height: 100%;
    /* Side columns give way before the main area does (WCAG 1.4.10 Reflow). */
    grid-template-columns: minmax(160px, 260px) minmax(300px, 1fr) minmax(220px, 340px);
    grid-template-rows: 36px minmax(0, 1fr) 24px;
    grid-template-areas:
      "header header header"
      "sidebar main agents"
      "footer footer footer";
  }
  header {
    grid-area: header;
    min-width: 0;
    overflow: hidden;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid #2a2a31;
  }
  h1 {
    margin: 0;
    font-size: 14px;
  }
  .project {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .muted {
    font-weight: normal;
    color: #9a9aa6;
  }
  aside {
    grid-area: sidebar;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    border-right: 1px solid #2a2a31;
  }
  .sidebar-tabs {
    display: flex;
    flex: none;
    border-bottom: 1px solid #2a2a31;
  }
  .sidebar-tabs button {
    flex: 1;
    padding: 4px 8px;
    font: inherit;
    color: #9a9aa6;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
  }
  .sidebar-tabs button[aria-pressed="true"] {
    color: #e6e6e6;
    border-bottom-color: #4a6bb0;
  }
  /* Forced colours replace both colours above: a system colour and a
     heavier weight tell the shown view apart. */
  @media (forced-colors: active) {
    .sidebar-tabs button {
      border-bottom-color: Canvas;
    }
    .sidebar-tabs button[aria-pressed="true"] {
      font-weight: 600;
      border-bottom-color: Highlight;
    }
  }
  .sidebar-view {
    flex: 1;
    min-height: 0;
  }
  .sidebar-view[hidden] {
    display: none;
  }
  .agents {
    grid-area: agents;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    border-right: none;
    border-left: 1px solid #2a2a31;
  }
  main {
    grid-area: main;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }
  .view {
    flex: 1;
    min-height: 0;
  }
  .file[hidden] {
    display: none;
  }
  .file {
    height: 100%;
  }
  footer {
    grid-area: footer;
    display: flex;
    align-items: center;
    padding: 0 12px;
    font-size: 12px;
    color: #9a9aa6;
    border-top: 1px solid #2a2a31;
  }
  .refused {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 max(16px, calc(50vw - 280px));
    background: #16161a;
  }
  .refused h2 {
    margin: 0 0 12px;
    font-size: 18px;
  }
  .refused p {
    margin: 0 0 8px;
    line-height: 1.5;
  }
  .error {
    color: #ff6b6b;
  }
  /* About 200% zoom on a laptop: agents and approvals move under the main area. */
  @media (max-width: 900px) {
    .shell {
      grid-template-columns: minmax(140px, 220px) minmax(0, 1fr);
      grid-template-rows: 36px minmax(240px, 1fr) minmax(160px, 35%) 24px;
      grid-template-areas:
        "header header"
        "sidebar main"
        "agents agents"
        "footer footer";
    }
    .agents {
      border-left: none;
      border-top: 1px solid #2a2a31;
    }
  }
  /* Narrower still: one column; the page scrolls down, never sideways. */
  @media (max-width: 600px) {
    .shell {
      height: auto;
      min-height: 100%;
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: 36px minmax(320px, 70vh) minmax(200px, 40vh) minmax(200px, 40vh) 24px;
      grid-template-areas:
        "header"
        "main"
        "sidebar"
        "agents"
        "footer";
    }
    aside {
      border-right: none;
      border-top: 1px solid #2a2a31;
    }
  }
</style>
