<script lang="ts">
// The window's frame (docs/specs/quiet-style.md): the title bar, the icon
// rail, the rail's panel and Agents on either side of the main area, and a
// quiet status bar. The side panels resize, fold to a strip and swap sides
// (layout.svelte.ts). The window itself never scrolls: every frame area
// clips, and the panels inside scroll on their own.
import { untrack } from "svelte";
import AgentsPanel from "../agents/AgentsPanel.svelte";
import { agentSessions } from "../agents/sessions.svelte";
import ClaudeSetup from "../agents/setup/ClaudeSetup.svelte";
import Approvals from "../approvals/Approvals.svelte";
import { approvalQueue } from "../approvals/queue.svelte";
import BrowserPane from "../browser/BrowserPane.svelte";
import Editor from "../editor/Editor.svelte";
import { persistOpenFiles } from "../editor/open-files.svelte";
import Tabs from "../editor/Tabs.svelte";
import Git from "../git/Git.svelte";
import GitDiff from "../git/GitDiff.svelte";
import { registerCommand } from "../keys/commands";
import Keybindings from "../keys/Keybindings.svelte";
import type { CoreInfo } from "../lib/protocol";
import type { ConnectionStatus, Transport } from "../lib/transport";
import LanguageServers from "../lsp/LanguageServers.svelte";
import ContextMenu from "../menu/ContextMenu.svelte";
import Notifications from "../notify/Notifications.svelte";
import Palette from "../palette/Palette.svelte";
import Search from "../search/Search.svelte";
import Terminals from "../terminal/Terminals.svelte";
import Tree from "../tree/Tree.svelte";
import Viewer from "../viewers/Viewer.svelte";
import { provideOvra } from "./context";
import Icon from "./Icon.svelte";
import { rememberProject, restoreProject } from "./last-project";
import { Layout, type Panel } from "./layout.svelte";
import Rail from "./Rail.svelte";
import Splitter from "./Splitter.svelte";
import TitleBar from "./TitleBar.svelte";
import { type SidebarView, Workspace } from "./workspace.svelte";

let { transport }: { transport: Transport } = $props();

const workspace = new Workspace();
// svelte-ignore state_referenced_locally
provideOvra({ transport, workspace });
const layout = new Layout();
// svelte-ignore state_referenced_locally
const sessions = agentSessions(transport);
// svelte-ignore state_referenced_locally
const approvals = approvalQueue(transport);

const VIEWS: Record<SidebarView, { name: string }> = {
  files: { name: "Files" },
  search: { name: "Search" },
  git: { name: "Git" },
};

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

$effect(() => registerCommand({ id: "layout.swap", title: "Swap side panels", run: () => layout.swap() }));

const view = $derived(VIEWS[workspace.sidebar]);
const capacity = $derived(workspace.project ? sessions.capacity.get(workspace.project.id) : undefined);
/** Folded panels. Agents unfolds while a request waits for a person, so approvals stay in sight. */
const folded = $derived({
  side: !!workspace.project && layout.side.collapsed,
  agents: layout.agents.collapsed && approvals.pending.length === 0,
});

/** The start screen has no Agents column, unless a request of an earlier project waits there. */
const solo = $derived(!workspace.project && approvals.pending.length === 0);

/**
 * The grid's tracks and areas; narrow windows replace them in the stylesheet.
 * The rail sits on the edge of its panel's side, and a folded rail's panel
 * leaves no strip: the rail's buttons unfold it.
 */
const grid = $derived.by(() => {
  const left = layout.left;
  const right: Panel = left === "side" ? "agents" : "side";
  const track = (panel: Panel) =>
    folded[panel] ? (panel === "side" ? "0" : "48px") : `minmax(0, ${layout[panel].width}px)`;
  const handle = (panel: Panel) => (folded[panel] ? "0" : "8px");
  const main = "minmax(320px, 1fr)";
  let tracks = `${track(left)} ${handle(left)} ${main} ${handle(right)} ${track(right)}`;
  let row = `${left} ${left}-handle main ${right}-handle ${right}`;
  if (!workspace.project) {
    // The start screen: the start page takes the main area's place, and there is no rail.
    tracks = solo
      ? `0 0 ${main} 0 0`
      : left === "side"
        ? `0 0 ${main} ${handle(right)} ${track(right)}`
        : `${track(left)} ${handle(left)} ${main} 0 0`;
    row = solo
      ? "side side side side side"
      : left === "side"
        ? "side side side agents-handle agents"
        : "agents agents-handle side side side";
    return `--tracks: 0 ${tracks}; --row: "rail ${row}";`;
  }
  return layout.left === "side"
    ? `--tracks: 52px ${tracks}; --row: "rail ${row}";`
    : `--tracks: ${tracks} 52px; --row: "${row} rail";`;
});

const PANELS: Panel[] = ["side", "agents"];

function edge(panel: Panel): "left" | "right" {
  return layout.left === panel ? "left" : "right";
}

/** Folds `panel`. The rail's panel leaves no strip, so focus goes to the rail button that unfolds it. */
function fold(panel: Panel, name: string): void {
  layout.setCollapsed(panel, true);
  if (panel === "side") document.querySelector<HTMLElement>(`nav[aria-label="Views"] [aria-label="${name}"]`)?.focus();
}
</script>

{#snippet controls(panel: Panel, name: string)}
  <span class="controls">
    <button
      type="button"
      title="Move to the other side"
      aria-label={`Move ${name} to the other side`}
      onclick={() => layout.swap()}><Icon name="swap" small /></button
    >
    <button
      type="button"
      title="Collapse"
      aria-label={`Collapse ${name}`}
      onclick={() => fold(panel, name)}><Icon name="collapse" small /></button

    >
  </span>
{/snippet}

{#snippet agentsControls()}
  {@render controls("agents", "Agents")}
{/snippet}

<Keybindings />
<div class="shell" class:start={!workspace.project} class:solo class:rail-right={layout.swapped} style={grid}>
  <div class="title-area"><TitleBar /></div>
  <div class="rail-area"><Rail {layout} /></div>
  <aside class="panel side" data-testid="sidebar" aria-label={workspace.project ? view.name : "Projects"}>
    <div class="body" class:own-head={workspace.sidebar === "files"} hidden={folded.side}>
      <!-- The file tree draws its own header; Search and Git get one here. -->
      {#if workspace.project}
        <div class="head">
          {#if workspace.sidebar !== "files"}<span class="name">{view.name}</span>{/if}
          {@render controls("side", view.name)}
        </div>
      {/if}
      <div class="sidebar-view" hidden={!!workspace.project && workspace.sidebar !== "files"}><Tree /></div>
      <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "search"}><Search /></div>
      <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "git"}><Git /></div>
      {#if !workspace.project}
        <!-- The start screen's second column: whether Claude is ready, before any project is open. -->
        <section class="claude-column" aria-labelledby="start-claude">
          <h2 id="start-claude">Claude</h2>
          <ClaudeSetup open />
        </section>
      {/if}
    </div>
  </aside>
  {#each PANELS as panel (panel)}
    {#if !folded[panel] && (panel === "agents" || workspace.project)}
      <div class="handle {panel}-handle">
        <Splitter
          label={panel === "side" ? `Resize ${view.name}` : "Resize Agents"}
          width={layout[panel].width}
          edge={edge(panel)}
          onresize={(width) => layout.resize(panel, width)}
          onreset={() => layout.reset(panel)}
        />
      </div>
    {/if}
  {/each}
  <main data-testid="main" hidden={!workspace.project}>
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
  <aside class="panel agents" data-testid="agents" aria-label="Agents and approvals">
    {#if folded.agents}
      <div class="strip">
        <button
          type="button"
          title="Show Agents"
          aria-label={capacity?.working ? `Show Agents, ${capacity.working} working` : "Show Agents"}
          onclick={() => layout.setCollapsed("agents", false)}
          ><Icon name="agent" />{#if capacity?.working}<span class="badge" aria-hidden="true">{capacity.working}</span
            >{/if}</button
        >
      </div>
    {/if}
    <!-- One column, top to bottom: approvals (bounded, they scroll themselves),
         then Agents, whose list or transcript scrolls above a reply box that
         stays in sight. Agents draws its own header and puts the swap and
         collapse buttons in it. -->
    <div class="body column" hidden={folded.agents}>
      <Approvals />
      <AgentsPanel controls={agentsControls} />
    </div>
  </aside>
  <!-- The connection on the left; what runs for the open project on the right. -->
  <footer>
    <span
      class="status"
      data-testid="status"
      role="status"
      class:error={!status.ok}
      title={info ? `${info.name} ${info.version} (${info.mode})` : undefined}>{status.text}</span
    >
    <span class="footer-end"><LanguageServers /></span>
  </footer>
  <Notifications />
  <ContextMenu />
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
    position: relative;
    display: grid;
    height: 100%;
    /* Nothing in the frame scrolls, not even through scrollIntoView: only the panels' own lists do. */
    overflow: clip;
    background: var(--s0);
    grid-template-columns: var(--tracks);
    grid-template-rows: 36px minmax(0, 1fr) 28px;
    grid-template-areas:
      "title title title title title title"
      var(--row)
      "footer footer footer footer footer footer";
  }
  .title-area {
    grid-area: title;
    min-width: 0;
    overflow: clip;
  }
  .rail-area {
    grid-area: rail;
    display: flex;
    flex-direction: column;
  }
  .rail-area > :global(nav) {
    flex: 1;
  }
  .panel {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: clip;
    background: var(--s1);
  }
  .side {
    grid-area: side;
  }
  .agents {
    grid-area: agents;
  }
  /* The start page in the main area's tone, Claude in the side panels'. */
  .start .side {
    background: var(--s2);
  }
  .start .side > .body {
    flex-direction: row;
  }
  .claude-column {
    flex: none;
    width: 340px;
    padding: 64px 24px 24px;
    overflow-y: auto;
    background: var(--s1);
  }
  .claude-column h2 {
    margin: 0 0 12px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
  }
  .start .rail-area,
  .solo .agents,
  .solo .agents-handle {
    display: none;
  }

  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
  .body[hidden],
  .sidebar-view[hidden],
  main[hidden],
  .file[hidden] {
    display: none;
  }
  /* Every side panel's header: the file tree's and the Agents panel's have the same height and inset. */
  .head {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    height: 44px;
    padding: 8px 8px 4px 16px;
    font-weight: 600;
  }
  /* Chrome on demand: the panel's own buttons are faint until the header is pointed at or focused. */
  .controls {
    display: flex;
    flex: none;
    gap: 2px;
    margin-left: auto;
  }
  .controls button {
    color: var(--text-3);
  }
  :where(.head:hover, .own-head:hover, .column:hover) .controls button,
  .controls:focus-within button {
    color: var(--text-2);
  }
  /* Nothing in the column may be taller than the column. */
  .column {
    overflow: clip;
  }
  /* With nothing to approve, the approvals band takes no room. Focus in it
     (the shortcut, the last answer) opens it in the column's flow, above the
     Agents header instead of over it. It closes a moment after focus leaves,
     so the click that moved focus still lands where it was aimed. */
  .column > :global(section.approvals.idle) {
    max-height: 0;
    overflow: hidden;
    transition: max-height 0s 0.4s;
  }
  .column > :global(section.approvals.idle:focus-within) {
    max-height: 120px;
    transition: none;
  }
  .column > :global(section.approvals.idle > .body) {
    position: static;
    width: auto;
    height: auto;
    padding: 10px 12px;
    clip-path: none;
    white-space: normal;
    box-shadow: none;
  }
  .own-head {
    position: relative;
  }
  .own-head > .head {
    position: absolute;
    top: 0;
    right: 0;
    z-index: 1;
    padding-left: 0;
  }
  .own-head :global(.toolbar) {
    padding-right: 72px;
  }
  .strip button {
    color: var(--text-2);
  }
  .controls button,
  .strip button {
    position: relative;
    display: grid;
    place-items: center;
    padding: 0;
    background: none;
    border: 0;
    border-radius: 7px;
    cursor: pointer;
  }
  .controls button {
    width: 26px;
    height: 26px;
  }
  .controls button:hover,
  .strip button:hover {
    color: var(--text);
    background: var(--s3);
  }
  .strip {
    display: flex;
    justify-content: center;
    padding-top: 10px;
  }
  .strip button {
    width: 34px;
    height: 34px;
  }
  .badge {
    position: absolute;
    top: 0;
    right: -2px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font: 600 12px / 16px var(--font);
    color: var(--accent-ink);
    background: var(--accent);
    border-radius: 8px;
  }
  .sidebar-view {
    flex: 1;
    min-height: 0;
    /* A view that needs more room than a short window gives scrolls here, not the page. */
    overflow-y: auto;
  }
  .handle {
    min-width: 0;
    background: var(--s1);
  }
  .side-handle {
    grid-area: side-handle;
  }
  .agents-handle {
    grid-area: agents-handle;
  }
  main {
    grid-area: main;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: clip;
    background: var(--s2);
  }
  .view {
    flex: 1;
    min-height: 0;
  }
  .file {
    height: 100%;
  }
  footer {
    grid-area: footer;
    display: flex;
    align-items: center;
    gap: 18px;
    min-width: 0;
    padding: 0 16px;
    /* No clip: the language server list opens upwards from here. */
    font-size: 13px;
    color: var(--text-2);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .footer-end {
    display: flex;
    align-items: center;
    gap: 18px;
    margin-left: auto;
    min-width: 0;
  }
  .status::before {
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
  }
  .status.error {
    color: var(--danger);
  }
  .status.error::before {
    background: var(--danger);
  }
  .refused {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 max(16px, calc(50vw - 280px));
    background: var(--s1);
  }
  .refused h2 {
    margin: 0 0 12px;
    font-size: 20px;
    font-weight: 600;
  }
  .refused p {
    margin: 0 0 8px;
    color: var(--text-2);
  }
  @media (forced-colors: active) {
    .handle {
      border-inline: 1px solid CanvasText;
    }
  }
  /* About 200% zoom on a laptop: Agents moves under the main area and nothing resizes. */
  @media (max-width: 900px) {
    .shell,
    .shell.start {
      grid-template-columns: 52px minmax(0, 220px) minmax(0, 1fr);
      /* Both rows give way in a short window, so nothing falls under the window's edge. */
      grid-template-rows: 36px minmax(0, 1fr) minmax(0, 50%) 28px;
      grid-template-areas:
        "title title title"
        "rail side main"
        "rail agents agents"
        "footer footer footer";
    }
    .shell.start {
      grid-template-areas:
        "title title title"
        "side side side"
        "agents agents agents"
        "footer footer footer";
    }
    .shell.solo {
      grid-template-rows: 44px minmax(0, 1fr) 28px;
      grid-template-areas:
        "title title title"
        "side side side"
        "footer footer footer";
    }
    .handle {
      display: none;
    }
    .start .side > .body {
      flex-direction: column;
    }
    .claude-column {
      width: auto;
      padding-top: 24px;
    }

  }
  /* Narrower still: one column, the rail a row; the page scrolls down, never sideways. */
  @media (max-width: 600px) {
    .shell,
    .shell.start {
      height: auto;
      min-height: 100%;
      overflow-x: clip;
      overflow-y: visible;
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: 36px 44px minmax(320px, 70vh) minmax(200px, 40vh) minmax(200px, 40vh) 28px;
      grid-template-areas:
        "title"
        "rail"
        "main"
        "side"
        "agents"
        "footer";
    }
    .shell.start {
      grid-template-rows: 44px minmax(320px, auto) minmax(200px, 40vh) 28px;
      grid-template-areas:
        "title"
        "side"
        "agents"
        "footer";
    }
    .shell.solo {
      grid-template-rows: 44px minmax(320px, auto) 28px;
      grid-template-areas:
        "title"
        "side"
        "footer";
    }
    .rail-area > :global(nav) {
      flex-direction: row;
      padding: 0 8px;
    }
  }

</style>
