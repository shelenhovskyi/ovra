<script lang="ts">
import { untrack } from "svelte";
import Editor from "../editor/Editor.svelte";
import { persistOpenFiles } from "../editor/open-files.svelte";
import Tabs from "../editor/Tabs.svelte";
import Keybindings from "../keys/Keybindings.svelte";
import type { CoreInfo } from "../lib/protocol";
import type { ConnectionStatus, Transport } from "../lib/transport";
import Palette from "../palette/Palette.svelte";
import Tree from "../tree/Tree.svelte";
import Viewer from "../viewers/Viewer.svelte";
import { provideOvra } from "./context";
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

$effect(() => persistOpenFiles(transport, workspace));

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
    if (info) return { text: `Connected to ${info.name} ${info.version} (${info.mode})`, ok: true };
    if (error) return { text: `Core unavailable: ${error}`, ok: false };
    return { text: "Connecting…", ok: true };
  }
  if (wasOpen) return { text: "Connection to core lost, reconnecting…", ok: false };
  if (everClosed || error) return { text: `Core unavailable: ${error ?? "could not connect, retrying…"}`, ok: false };
  return { text: "Connecting…", ok: true };
});
</script>

<Keybindings />
<div class="shell">
  <header>
    <h1>Ovra</h1>
    <span class="project">{workspace.project?.name ?? "No project"}</span>
  </header>
  <aside data-testid="sidebar"><Tree /></aside>
  <main data-testid="main">
    <Tabs />
    <div class="view" id="ovra-tabpanel" role="tabpanel" aria-label={workspace.openPath ?? "No file open"}>
      {#if workspace.mode === "edit"}
        <Editor />
      {:else}
        <Viewer />
      {/if}
    </div>
  </main>
  <footer>
    <span data-testid="status" role="status" class:error={!status.ok}>{status.text}</span>
  </footer>
  {#if workspace.paletteOpen}
    <Palette />
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    font-family: system-ui, sans-serif;
    font-size: 13px;
    background: #16161a;
    color: #e6e6e6;
  }
  :global(#app) {
    height: 100%;
  }
  .shell {
    display: grid;
    height: 100%;
    grid-template-columns: 260px 1fr;
    grid-template-rows: 36px 1fr 24px;
    grid-template-areas:
      "header header"
      "sidebar main"
      "footer footer";
  }
  header {
    grid-area: header;
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
    color: #9a9aa6;
  }
  aside {
    grid-area: sidebar;
    overflow: hidden;
    border-right: 1px solid #2a2a31;
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
  footer {
    grid-area: footer;
    display: flex;
    align-items: center;
    padding: 0 12px;
    font-size: 12px;
    color: #9a9aa6;
    border-top: 1px solid #2a2a31;
  }
  .error {
    color: #ff6b6b;
  }
</style>
