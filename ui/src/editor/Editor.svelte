<script lang="ts">
// The editor for workspace.openPath (main area in "edit" mode). One view
// shows the active tab's buffer; buffers keep their own text, undo history
// and selection while they are not shown.
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { useOvra } from "../shell/context";
import type { Buffer } from "./buffers.svelte";
import { answerClose, tabs } from "./open-files.svelte";

const { workspace } = useOvra();

let host = $state<HTMLDivElement>();
let view: EditorView | null = null;
let buffer = $state<Buffer | null>(null);
/** Entering edit mode focuses the text; later tab switches keep focus in the tab strip. */
let entering = true;

$effect(() => {
  if (!host) return;
  const created = new EditorView({ parent: host });
  view = created;
  return () => {
    buffer?.detach();
    created.destroy();
    view = null;
  };
});

$effect(() => {
  const buffers = tabs.current?.buffers;
  const path = workspace.openPath;
  if (!buffers || !path || !host) {
    buffer = null;
    return;
  }
  const next = buffers.open(path);
  buffer = next;
  let live = true;
  void next.loaded.then(() => {
    if (!live || !view) return;
    next.attach(view);
    if (entering) view.focus();
    else focusUnlessInTabs();
    entering = false;
  });
  return () => {
    live = false;
    next.detach();
    // Nothing typed before the next buffer arrives may land in this one.
    view?.setState(EditorState.create());
  };
});

/** Focus goes to the text, except while the user moves through the tabs. */
function focusUnlessInTabs(): void {
  if (!document.activeElement?.closest('[role="tablist"], dialog')) view?.focus();
}

async function save(overwrite = false): Promise<void> {
  await buffer?.save(overwrite);
  view?.focus();
}

async function revert(): Promise<void> {
  await buffer?.revert();
  view?.focus();
}

function discard(path: string): void {
  tabs.closing = path;
  void answerClose(workspace, "discard");
}
</script>

<div class="editor" data-panel="Editor">
  {#if buffer}
    <div class="bar">
      <span class="path" data-testid="editor-path">{buffer.path}</span>
      {#if buffer.dirty}
        <span class="dirty" title="Unsaved changes" data-testid="editor-dirty">●<span class="visually-hidden">Unsaved changes</span></span>
      {/if}
      <span class="language" data-testid="editor-language"
        >{buffer.heavy ? "Plain text (large file, highlighting off)" : (buffer.language?.name ?? "Plain text")}</span
      >
      {#if buffer.status === "ready"}
        <button type="button" onclick={() => save()} disabled={!buffer.dirty}>Save</button>
      {/if}
    </div>
    {#if buffer.deleted}
      <div class="notice" role="alert">
        {buffer.path} was deleted on disk.
        <button type="button" onclick={() => save(true)}>Save as a new file</button>
        <button type="button" onclick={() => buffer && discard(buffer.path)}>Close without saving</button>
      </div>
    {:else if buffer.conflict}
      <div class="notice" role="alert">
        {buffer.path} changed on disk{buffer.dirty ? " while you have unsaved changes" : ""}.
        <button type="button" onclick={() => save(true)}>Overwrite with my version</button>
        <button type="button" onclick={revert}>Discard my changes and reload</button>
      </div>
    {:else if buffer.error}
      <div class="notice" role="alert">{buffer.error}</div>
    {/if}
    {#if buffer.status === "binary"}
      <p class="message">This is a binary file and cannot be edited here.</p>
    {:else if buffer.status === "too_large"}
      <p class="message">This file is too large to edit here.</p>
    {/if}
  {:else}
    <p class="message">No file open.</p>
  {/if}
  <div class="host" class:hidden={buffer?.status !== "ready"} bind:this={host}></div>
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
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
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
  .hidden {
    display: none;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
