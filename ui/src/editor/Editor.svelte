<script lang="ts">
// The editor for workspace.openPath (main area in "edit" mode). One view
// shows the active tab's buffer; buffers keep their own text, undo history
// and selection while they are not shown.
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { untrack } from "svelte";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { useOvra } from "../shell/context";
import type { TextSpot } from "../shell/workspace.svelte";
import type { Buffer } from "./buffers.svelte";
import { toggle } from "./commands";
import { answerClose, tabs } from "./open-files.svelte";

const { workspace } = useOvra();
const viewKeys = formatShortcut(liveKeys("Mod+E", inBrowserTab()), isApple());
const saveKeys = formatShortcut("Mod+S", isApple());

let host = $state<HTMLDivElement>();
let view: EditorView | null = null;
let buffer = $state<Buffer | null>(null);
/** The buffer the view shows, once it is attached. */
let shown = $state<Buffer | null>(null);
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
  const lsp = tabs.current?.lsp;
  const next = buffers.open(path);
  buffer = next;
  let live = true;
  void next.loaded.then(() => {
    const target = view;
    if (!live || !target) return;
    next.attach(target);
    if (entering) target.focus();
    else focusUnlessInTabs();
    entering = false;
    shown = next;
    // Showing a file is what starts its language server (ADR 0007).
    untrack(() => lsp?.showing(next, target));
  });
  return () => {
    live = false;
    shown = null;
    lsp?.hiding(next);
    next.detach();
    // Nothing typed before the next buffer arrives may land in this one.
    view?.setState(EditorState.create());
  };
});

// A search result (or anything else) asked for a spot in the shown file.
$effect(() => {
  const spot = workspace.reveal;
  if (!spot || !view || shown?.path !== spot.path || shown.status !== "ready") return;
  untrack(() => {
    if (view) select(view, spot);
    workspace.reveal = null;
  });
});

/** Selects `spot`, clamped to the text, and scrolls it to the middle. */
function select(target: EditorView, spot: TextSpot): void {
  const doc = target.state.doc;
  const line = doc.line(Math.min(Math.max(spot.line, 1), doc.lines));
  const at = (offset: number) => Math.min(line.from + Math.max(offset, 0), line.to);
  target.dispatch({
    selection: { anchor: at(spot.from), head: at(spot.to) },
    effects: EditorView.scrollIntoView(at(spot.from), { y: "center" }),
  });
  target.focus();
}

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
      <button type="button" title="Back to the read-only view ({viewKeys})" onclick={() => toggle(workspace)}
        >View <kbd>{viewKeys}</kbd></button
      >
      {#if buffer.status === "ready"}
        <button
          type="button"
          class="primary"
          title={buffer.dirty ? `Save (${saveKeys})` : "No unsaved changes"}
          onclick={() => save()}
          disabled={!buffer.dirty}>Save</button
        >
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
  button {
    padding: 1px 8px;
    font: inherit;
    font-size: 12px;
    color: #e6e6e6;
    background: none;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: #2a2a31;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .primary {
    background: #2f3d5c;
    border-color: #4a6bb0;
  }
  .primary:hover:not(:disabled) {
    background: #3a4b70;
  }
  /* Disabled stays readable (#b4b4c0 on the bar, 8.8:1); the flat border says it is off. */
  button:disabled {
    color: #b4b4c0;
    background: none;
    border-color: #2a2a31;
    cursor: default;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    color: #9a9aa6;
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
    color: #9a9aa6;
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
