<script lang="ts">
// The window's frame (docs/specs/quiet-style.md): the title bar, the icon
// rail, the rail's panel and Agents on either side of the main area, and a
// quiet status bar. The side panels resize, fold to a strip and swap sides
// (layout.svelte.ts). The window itself never scrolls: every frame area
// clips, and the panels inside scroll on their own.
import { tick, untrack } from "svelte";
import AgentsPanel from "../agents/AgentsPanel.svelte";
import { agentSessions } from "../agents/sessions.svelte";
import Approvals from "../approvals/Approvals.svelte";
import { approvalQueue } from "../approvals/queue.svelte";
import AuditLog from "../audit/AuditLog.svelte";
import BrowserPane from "../browser/BrowserPane.svelte";
import FileStatus from "../editor/FileStatus.svelte";
import { persistOpenFiles } from "../editor/open-files.svelte";
import FocusBar from "../focus/FocusBar.svelte";
import FocusView from "../focus/FocusView.svelte";
import { FOCUS_KEYS } from "../focus/keys";
import { focusMode } from "../focus/mode.svelte";
import Git from "../git/Git.svelte";
import GitDiff from "../git/GitDiff.svelte";
import Inbox from "../inbox/Inbox.svelte";
import { ReviewInbox } from "../inbox/reviews.svelte";
import { listCommands, registerCommand } from "../keys/commands";
import { registerContextKeys } from "../keys/context-keys";
import Keybindings from "../keys/Keybindings.svelte";
import type { CoreInfo } from "../lib/protocol";
import type { ConnectionStatus, Transport } from "../lib/transport";
import LanguageServers from "../lsp/LanguageServers.svelte";
import ContextMenu from "../menu/ContextMenu.svelte";
import Notifications from "../notify/Notifications.svelte";
import Palette from "../palette/Palette.svelte";
import Search from "../search/Search.svelte";
import { serversDock } from "../servers/dock.svelte";
import ServersDock from "../servers/ServersDock.svelte";
import ClaudePane from "../settings/ClaudePane.svelte";
import { registerSettingsCommands } from "../settings/commands";
import Settings from "../settings/Settings.svelte";
import Terminals from "../terminal/Terminals.svelte";
import Tree from "../tree/Tree.svelte";
import { provideOvra } from "./context";
import EditorGroup from "./EditorGroup.svelte";
import Icon from "./Icon.svelte";
import { rememberProject, restoreProject } from "./last-project";
import { Layout, MIN_WIDTH, type Panel } from "./layout.svelte";
import { mainView, VIEW_TITLES } from "./main-view.svelte";
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
// svelte-ignore state_referenced_locally
const reviews = new ReviewInbox(transport);
$effect(() => () => reviews.dispose());

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

// Opening a file or a diff shows it: a view in the main area makes way.
$effect(() => {
  void [workspace.openPath, workspace.gitDiff];
  untrack(() => mainView.close(false));
});

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

$effect(() => {
  const offSettings = registerSettingsCommands();
  const off = [
    registerCommand({
      id: "focus.toggle",
      title: "Focus: agents only",
      category: "View",
      keywords: ["zen", "distraction", "quiet", "ide"],
      keys: FOCUS_KEYS,
      run: async () => {
        focusMode.toggle();
        if (!workspace.project) return;
        // What had focus may be hidden now: put it where the person can go on.
        await tick();
        const to = focusMode.on
          ? document.querySelector<HTMLElement>("[data-testid=focus-open-ide]")
          : document.getElementById("ovra-tabpanel");
        to?.focus();
      },
    }),
    registerCommand({ id: "layout.swap", title: "Swap side panels", category: "View", run: () => layout.swap() }),
    registerCommand({ id: "layout.reset", title: "Reset layout", category: "View", run: () => layout.resetAll() }),
    registerCommand({
      id: "layout.toggle_side",
      title: "Show or hide the sidebar",
      category: "View",
      keywords: ["collapse", "panel"],
      keys: "Mod+0",
      run: () => toggle("side"),
    }),
    registerCommand({
      id: "layout.toggle_agents",
      title: "Show or hide Agents",
      category: "View",
      keywords: ["collapse", "panel"],
      keys: "Alt+Mod+0",
      run: () => toggle("agents"),
    }),
    registerCommand({
      id: "audit.toggle",
      title: "Audit log",
      category: "View",
      keywords: ["history"],
      // The menu bar's key (docs/design/patterns.md).
      keys: "Alt+Mod+L",
      run: () => mainView.toggle("audit"),
    }),
    // The keys of its main-area views; each panel declares its own.
    registerContextKeys([{ keys: "Escape", title: "Close Settings, Reviews or the Audit log", where: "Views" }]),
  ];
  return () => {
    for (const unregister of off) unregister();
    offSettings();
  };
});

const view = $derived(VIEWS[workspace.sidebar]);
/** Focus (docs/specs/focus-mode.md) shows once a project is open; the IDE stays mounted under it. */
const focusing = $derived(focusMode.on && !!workspace.project);
const capacity = $derived(workspace.project ? sessions.capacity.get(workspace.project.id) : undefined);
/** The requests that waited when the person folded Agents; a new one unfolds it again. */
let seenWhenFolded = $state<string[]>([]);
/** Folded panels. Agents unfolds when a new request waits for a person, so approvals come into sight. */
const folded = $derived({
  side: !!workspace.project && layout.side.collapsed,
  agents: layout.agents.collapsed && approvals.pending.every((r) => seenWhenFolded.includes(r.id)),
});
/** Requests waiting while Agents is folded: the title bar and the strip say how many. */
const waiting = $derived(folded.agents ? approvals.pending.length : 0);

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
    folded[panel] ? (panel === "side" ? "0" : "48px") : `minmax(0, ${layout.width(panel)}px)`;
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
    return `--tracks: 0 ${tracks}; --shell-row: "rail ${row}";`;
  }
  return layout.left === "side"
    ? `--tracks: 52px ${tracks}; --shell-row: "rail ${row}";`
    : `--tracks: ${tracks} 52px; --shell-row: "${row} rail";`;
});

const PANELS: Panel[] = ["side", "agents"];

/** The handle between the editor groups. */
const GROUP_HANDLE = 8;
/** How wide the two editor groups are together, for their splitter. */
let groupsWidth = $state(0);
let groupsBox = $state<HTMLElement>();
$effect(() => {
  const box = groupsBox;
  if (!box || typeof ResizeObserver === "undefined") return;
  const observer = new ResizeObserver(() => {
    groupsWidth = box.clientWidth;
  });
  observer.observe(box);
  return () => observer.disconnect();
});
const groupsRoom = $derived(Math.max(0, groupsWidth - GROUP_HANDLE));
/** The tracks of the tab row and the files while the main area is split; both use them, so tabs sit above their file. */
const groupColumns = $derived(`minmax(0, ${layout.groups}fr) ${GROUP_HANDLE}px minmax(0, ${1 - layout.groups}fr)`);

function resizeGroups(width: number): void {
  if (groupsRoom <= 0) return;
  layout.resizeGroups(Math.min(Math.max(width, MIN_WIDTH), groupsRoom - MIN_WIDTH) / groupsRoom);
}

function edge(panel: Panel): "left" | "right" {
  return layout.left === panel ? "left" : "right";
}

/**
 * Folds or unfolds `panel`. Focus inside a panel that folds goes to what
 * unfolds it: the rail's button for the rail's panel, the Agents strip.
 */
async function toggle(panel: Panel): Promise<void> {
  const fold = !folded[panel];
  const area = document.querySelector(panel === "side" ? "[data-testid=sidebar]" : "[data-testid=agents]");
  const inside = !!area?.contains(document.activeElement);
  if (panel === "agents") seenWhenFolded = fold ? approvals.pending.map((r) => r.id) : [];
  layout.setCollapsed(panel, fold);
  if (!fold || !inside) return;
  await tick();
  const back =
    panel === "side"
      ? document.querySelector<HTMLElement>(`nav[aria-label="Views"] [aria-label="${view.name}"]`)
      : document.querySelector<HTMLElement>("[data-testid=agents] .strip button");
  back?.focus();
}

/** A press on a file's tab while a view covers the files: back to that file, even the one already open. */
function backToFiles(event: MouseEvent): void {
  if (mainView.current && event.target instanceof Element && event.target.closest("[role=tab]")) mainView.close(false);
}

/** Bumped to ask again whether Claude is ready: the person may have fixed it in a terminal meanwhile. */
let claudeCheck = $state(0);

/** The window is back: on the start screen, Claude's check runs again. */
function recheckClaude(): void {
  if (!workspace.project) claudeCheck++;
}

/** Shows Agents and moves to the first waiting request. */
async function showApprovals(): Promise<void> {
  if (folded.agents) await toggle("agents");
  await tick();
  void listCommands()
    .find((c) => c.id === "approvals.focus")
    ?.run();
}
</script>

<svelte:window bind:innerWidth={layout.viewport} onfocus={recheckClaude} />
<Keybindings />
<div
  class="shell"
  class:start={!workspace.project}
  class:viewing={!workspace.project && !!mainView.current}
  class:solo class:rail-right={layout.swapped} class:focus={focusing} style={grid}>
  <div class="title-area">
    {#if focusing}<FocusBar />{/if}
    <TitleBar {layout} shown={{ side: !folded.side, agents: !folded.agents }} {waiting} {toggle} {showApprovals} inbox={reviews} />
  </div>
  <div class="rail-area"><Rail {layout} /></div>
  <aside class="panel side" data-testid="sidebar" aria-label={workspace.project ? view.name : "Projects"}>
    <div class="body" hidden={folded.side}>
      <!-- The file tree draws its own header; Search and Git get one here. -->
      {#if workspace.project && workspace.sidebar !== "files"}
        <div class="head"><span class="name">{view.name}</span></div>
      {/if}
      <div class="sidebar-view" hidden={!!workspace.project && workspace.sidebar !== "files"}><Tree /></div>
      <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "search"}><Search /></div>
      <div class="sidebar-view" hidden={!workspace.project || workspace.sidebar !== "git"}><Git /></div>
      {#if !workspace.project}
        <!-- The start screen's second column: whether Claude is ready, before any project is open. -->
        <section class="claude-column" aria-labelledby="start-claude">
          <h2 id="start-claude">Claude</h2>
          {#key claudeCheck}<ClaudePane />{/key}
        </section>
      {/if}
    </div>
  </aside>
  {#each PANELS as panel (panel)}
    {#if !folded[panel] && (panel === "agents" || workspace.project)}
      <div class="handle {panel}-handle">
        <Splitter
          label={panel === "side" ? `Resize ${view.name}` : "Resize Agents"}
          width={layout.width(panel)}
          max={layout.max(panel)}
          edge={edge(panel)}
          onresize={(width) => layout.resize(panel, width)}
          onreset={() => layout.reset(panel)}
        />
      </div>
    {/if}
  {/each}
  <main data-testid="main" hidden={!workspace.project && !mainView.current}>
    <!-- The open files' tabs stay in sight under a view (Settings, Audit log,
         Reviews), which shows as one more tab: a file's tab brings it back. -->
    <div
      class="tab-row"
      class:covered={!!mainView.current}
      class:split={!!workspace.other}
      style:--groups={groupColumns}
    >
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="tab-strip" onclickcapture={backToFiles}><EditorGroup group={0} part="tabs" /></div>
      {#if workspace.other}
        <span aria-hidden="true"></span>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="tab-strip" onclickcapture={backToFiles}><EditorGroup group={1} part="tabs" /></div>
      {/if}
      {#if mainView.current}
        {@const title = VIEW_TITLES[mainView.current]}
        <div class="view-tab" data-testid="view-tab">
          <span class="view-name">{title}</span>
          <button
            type="button"
            class="view-close"
            aria-label={`Close ${title}`}
            title="Close (Escape)"
            aria-keyshortcuts="Escape"
            onclick={() => mainView.close()}><Icon name="close" small /></button
          >
        </div>
      {/if}
    </div>
    <!-- A view takes the files' place; they stay mounted under it. -->
    <div class="files" hidden={!!mainView.current}>
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
      <!-- One or two editor groups side by side, with a handle between them. -->
      <div
        class="file"
        class:split={!!workspace.other}
        hidden={!!gitDiff}
        style:--groups={groupColumns}
        bind:this={groupsBox}
      >
        <EditorGroup group={0} part="file" />
        {#if workspace.other}
          <div class="handle group-handle">
            <Splitter
              label="Resize editor groups"
              width={Math.round(layout.groups * groupsRoom)}
              max={Math.max(MIN_WIDTH, groupsRoom - MIN_WIDTH)}
              edge="left"
              onresize={resizeGroups}
              onreset={() => layout.resizeGroups(0.5)}
            />
          </div>
          <EditorGroup group={1} part="file" />
        {/if}
      </div>
    </div>
    </div>
    {#if mainView.current === "settings"}<Settings />{:else if mainView.current === "audit"}<AuditLog />{/if}
    <!-- Reviews stays mounted while closed: it keeps the choice and drafts, and agents' asks. -->
    <Inbox inbox={reviews} />
    <BrowserPane />
    <!-- The bottom dock: terminals and, next to them, the dev servers that run. -->
    <div class="dock">
      <Terminals />
      <ServersDock />
    </div>
  </main>
  <aside class="panel agents" data-testid="agents" aria-label="Agents and approvals">
    {#if folded.agents && !focusing}
      <div class="strip">
        <button
          type="button"
          title="Show Agents"
          aria-label={[
            "Show Agents",
            capacity?.working ? `${capacity.working} working` : "",
            waiting ? `${waiting} waiting for approval` : "",
          ]
            .filter(Boolean)
            .join(", ")}
          onclick={() => void toggle("agents")}
          ><Icon name="agent" />{#if waiting}<span class="badge warn" aria-hidden="true">{waiting}</span
            >{:else if capacity?.working}<span class="badge" aria-hidden="true">{capacity.working}</span>{/if}</button
        >
      </div>
    {/if}
    <!-- One column, top to bottom: approvals (bounded, they scroll themselves),
         then Agents, whose list or transcript scrolls above a reply box that
         stays in sight. Agents draws its own header; folding and swapping
         are the title bar's and the palette's. -->
    <div class="body column" hidden={folded.agents && !focusing}>
      <Approvals offerAlways={!focusing} />
      {#if focusing}<FocusView />{/if}
      <!-- Hidden, not unmounted, in Focus: its commands, drafts and transcripts stay. -->
      <div class="agents-panel" hidden={focusing}><AgentsPanel /></div>
    </div>
  </aside>
  <!-- Only what needs a person: the connection when it is lost (screen
       readers still hear it come and go), and on the right the open file's
       quiet facts and what runs for the open project. -->
  <footer>
    <span
      class="status"
      data-testid="status"
      role="status"
      class:error={!status.ok}
      class:quiet={status.ok}
      title={info ? `${info.name} ${info.version} (${info.mode})` : undefined}>{status.text}</span
    >
    <span class="footer-end">
      <!-- Quiet facts about the open file, while it is what the main area shows. -->
      {#if !gitDiff && !mainView.current}<FileStatus />{/if}
      {#if serversDock.running > 0 && !serversDock.shown}
        <button type="button" class="running" onclick={() => (serversDock.shown = true)}
          >{serversDock.running === 1 ? "1 server running" : `${serversDock.running} servers running`}</button
        >
      {/if}
      <LanguageServers />
    </span>
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
      var(--shell-row)
      "footer footer footer footer footer footer";
  }
  .title-area {
    grid-area: title;
    z-index: 2;
    min-width: 0;
    /* Sideways only: the title bar's tooltips hang below it. */
    overflow-x: clip;
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
  .files[hidden],
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
  .strip button {
    color: var(--text-2);
  }
  .agents-panel {
    display: contents;
  }
  .agents-panel[hidden] {
    display: none;
  }

  /* Focus: only the agents' work, in one centred grid; the IDE stays mounted, hidden. */
  .shell.focus {
    height: 100%;
    overflow: clip;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: 36px minmax(0, 1fr) 28px;
    grid-template-areas:
      "title"
      "agents"
      "footer";
  }
  .focus .rail-area,
  .focus .side,
  .focus .handle,
  .focus main,
  .focus .title-area > :global(header.title) {
    display: none;
  }
  .focus .agents {
    background: var(--s1);
    overflow-y: auto;
  }
  .focus .column {
    display: grid;
    flex: none;
    box-sizing: border-box;
    width: min(100%, 980px);
    margin: 0 auto;
    padding: 24px 20px 32px;
    overflow: visible;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
    grid-template-areas:
      "head head"
      "approvals status"
      "reviews status"
      ". status";
    grid-template-rows: auto auto auto 1fr;
    gap: 16px 32px;
    align-content: start;
  }
  .focus .column > :global(section.approvals:not(.idle)) {
    grid-area: approvals;
    max-height: none;
    padding: 0;
  }
  @media (max-width: 760px) {
    .focus .column {
      grid-template-columns: minmax(0, 1fr);
      grid-template-areas:
        "head"
        "approvals"
        "reviews"
        "status";
    }
  }
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
  .badge.warn {
    color: var(--s0);
    background: var(--warn);
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
  .tab-row {
    display: flex;
    flex: none;
    align-items: flex-end;
    min-width: 0;
    background: var(--s1);
  }
  .tab-strip {
    flex: 0 1 auto;
    min-width: 0;
  }
  /* Under a view, the open file's tab is no longer the selected one: the view's is. */
  .covered :global(.tabs .tab.active) {
    font-weight: 400;
    color: var(--text-2);
    background: none;
  }
  .view-tab {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    height: 32px;
    margin: 4px 8px 0;
    padding: 0 2px 0 12px;
    font-weight: 500;
    background: var(--s2);
    border-radius: var(--r-md) var(--r-md) 0 0;
    user-select: none;
  }
  /* After the file tabs, the same gap as between them. */
  .tab-strip:has(:global(.tabs)) + .view-tab {
    margin-left: -6px;
  }
  .view-close {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .view-close:hover {
    color: var(--text);
    background: var(--s3);
  }
  .files {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
  .view {
    flex: 1;
    min-height: 0;
  }
  /* The start screen with a view open (Settings, say): the view takes the start page's place. */
  .viewing > .side {
    display: none;
  }
  .viewing > main {
    grid-area: side;
  }
  .file {
    height: 100%;
  }
  .split {
    display: grid;
    grid-template-columns: var(--groups) auto;
  }
  .file.split {
    grid-template-columns: var(--groups);
  }
  .group-handle {
    background: var(--s2);
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
  /* Terminals and servers share the dock side by side; it takes no room while both are hidden. */
  .dock {
    display: flex;
    flex: 0 0 40%;
    gap: 2px;
    min-height: 120px;
    background: var(--s1);
  }
  .dock:not(:has(> :not([hidden]))) {
    display: none;
  }
  .dock > :global(section) {
    flex: 1 1 0;
    min-width: 0;
  }
  .running {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px;
    font: inherit;
    color: var(--text-2);
    background: none;
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .running:hover {
    color: var(--text);
    background: var(--s3);
  }
  .running::before {
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
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
  .status.quiet {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
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
