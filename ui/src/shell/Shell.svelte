<script lang="ts">
import Editor from "../editor/Editor.svelte";
import type { CoreInfo } from "../lib/protocol";
import type { Transport } from "../lib/transport";
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

$effect(() => {
  transport
    .call({ method: "info.get" })
    .then((response) => {
      info = response.data;
    })
    .catch((err: unknown) => {
      error = err instanceof Error ? err.message : String(err);
    });
});
</script>

<div class="shell">
  <header>
    <h1>Ovra</h1>
    <span class="project">{workspace.project?.name ?? "No project"}</span>
  </header>
  <aside data-testid="sidebar"><Tree /></aside>
  <main data-testid="main">
    {#if workspace.mode === "edit"}
      <Editor />
    {:else}
      <Viewer />
    {/if}
  </main>
  <footer>
    {#if info}
      <span data-testid="status">Connected to {info.name} {info.version} ({info.mode})</span>
    {:else if error}
      <span data-testid="status" class="error">Core unavailable: {error}</span>
    {:else}
      <span data-testid="status">Connecting…</span>
    {/if}
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
    overflow: hidden;
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
