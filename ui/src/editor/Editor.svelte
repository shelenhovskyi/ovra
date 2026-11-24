<script lang="ts">
// The editor for workspace.openPath (main area in "edit" mode).
import { registerCommand } from "../keys/commands";
import { useOvra } from "../shell/context";
import { highlighter } from "./highlight/highlighter";
import { EditorSession } from "./session.svelte";

const { transport, workspace } = useOvra();

let host = $state<HTMLDivElement>();
let session = $state<EditorSession | null>(null);

$effect(() => {
  const project = workspace.project;
  const path = workspace.openPath;
  if (!host || !project || !path) return;
  const opened = new EditorSession({ transport, project: project.id, path, parent: host, highlighter });
  session = opened;
  void opened.load();
  return () => opened.destroy();
});

$effect(() => registerCommand({ id: "editor.save", title: "Save file", keys: "Mod+S", run: () => session?.save() }));
</script>

<div class="editor" data-panel="Editor">
  {#if session}
    <div class="bar">
      <span class="path" data-testid="editor-path">{workspace.openPath}</span>
      {#if session.dirty}<span class="dirty" title="Unsaved changes" data-testid="editor-dirty">●</span>{/if}
      <span class="language">{session.language?.name ?? "Plain text"}</span>
      <button type="button" onclick={() => session?.save()} disabled={!session.dirty}>Save</button>
    </div>
    {#if session.conflict}
      <div class="notice" role="alert">
        The file changed on disk.
        <button type="button" onclick={() => session?.save(true)}>Overwrite</button>
        <button type="button" onclick={() => session?.reload()}>Reload from disk</button>
      </div>
    {:else if session.error}
      <div class="notice" role="alert">{session.error}</div>
    {/if}
    {#if session.status === "binary"}
      <p class="message">This is a binary file and cannot be edited here.</p>
    {:else if session.status === "too_large"}
      <p class="message">This file is too large to edit here.</p>
    {/if}
  {:else}
    <p class="message">No file open.</p>
  {/if}
  <div class="host" bind:this={host}></div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    border-bottom: 1px solid #2a2a31;
    color: #9a9aa6;
  }
  .path {
    color: #e6e6e6;
  }
  .language {
    margin-left: auto;
  }
  .notice {
    padding: 6px 12px;
    background: #3a2a1a;
    color: #ffd7a8;
  }
  .message {
    padding: 12px;
    color: #6f6f7a;
  }
  .host {
    flex: 1;
    min-height: 0;
  }
</style>
