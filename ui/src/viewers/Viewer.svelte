<script lang="ts">
// Shows workspace.openPath in view mode: markdown, images, diffs, plain text.
// The bar above the file has Edit, so it is always in sight; typing into the
// view says that it is read-only and how to edit.
import { toggle } from "../editor/commands";
import { serverFor } from "../editor/lsp/clients";
import { unsavedText } from "../editor/open-files.svelte";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { CoreError } from "../lib/transport";
import { languageServers } from "../lsp/servers.svelte";
import { useOvra } from "../shell/context";
import ImageView from "./ImageView.svelte";
import Lines from "./Lines.svelte";
import { formatSize, type Loaded, loadView, touches } from "./load";
import MarkdownView from "./MarkdownView.svelte";
import PatchView from "./PatchView.svelte";
import StartPanel from "./StartPanel.svelte";

const { transport, workspace } = useOvra();
const lsp = languageServers(transport);
// The viewer has no language features: say where they are.
const lspHint = $derived(workspace.openPath ? serverFor(workspace.openPath, lsp.status)?.name : undefined);
const editKeys = formatShortcut(liveKeys("Mod+E", inBrowserTab()), isApple());

let loaded = $state<Loaded | null>(null);
// Unsaved editor text wins over the file on disk, so the preview matches what will be saved.
let unsaved = $derived(
  workspace.project && workspace.openPath ? unsavedText(workspace.project.id, workspace.openPath) : null,
);
let shown = $derived.by(() => {
  if (!loaded || !("view" in loaded)) return loaded;
  const { view } = loaded;
  const textual = view.kind === "markdown" || view.kind === "patch" || view.kind === "text";
  return unsaved !== null && textual ? { ...loaded, view: { ...view, text: unsaved } } : loaded;
});
let notice = $state("");
let root = $state<HTMLElement | null>(null);
let content = $state<HTMLElement | null>(null);
// Whether keyboard focus was in the viewer, so it can follow to the next file
// (the focused link or button is gone once the content changes).
let hadFocus = false;

$effect(() => {
  const project = workspace.project?.id;
  const path = workspace.openPath;
  loaded = null;
  notice = "";
  if (!project || !path) return;
  let latest = 0;
  const load = () => {
    const seq = ++latest;
    void loadView(transport, project, path).then((result) => {
      if (seq === latest) loaded = result;
    });
  };
  load();
  const off = transport.onEvent((event) => {
    if (touches(event, project, path)) load();
  });
  return () => {
    latest = -1;
    off();
  };
});

$effect(() => {
  if (!loaded || !content || !hadFocus) return;
  const active = document.activeElement;
  if (active === null || active === document.body || !root?.contains(active)) {
    const target = content.querySelector<HTMLElement>("[tabindex='0']") ?? content;
    target.focus();
  }
});

/** Text kinds open in the editor; images and files too big or binary do not. */
const editable = $derived(!loaded || "error" in loaded || ["markdown", "patch", "text"].includes(loaded.view.kind));

/** A printable key typed into the read-only view: say so instead of dropping it. */
function ontyping(event: KeyboardEvent): void {
  if (event.key.length !== 1 || event.key === " " || event.ctrlKey || event.metaKey || event.altKey) return;
  if (event.target instanceof HTMLElement && event.target.closest("input, textarea, select, [contenteditable]")) return;
  notice = editable
    ? `This is a read-only view. Press Edit (${editKeys}) to change this file.`
    : "This file cannot be edited here.";
}

async function openExternal() {
  const project = workspace.project?.id;
  const path = workspace.openPath;
  if (!project || !path) return;
  try {
    await transport.call({ method: "open.external", params: { project, path, line: null } });
    notice = `Opened ${path} in its default app.`;
  } catch (err) {
    // The core says why, for example that the file could run as a program.
    const why = err instanceof CoreError ? `: ${err.error.message}` : "";
    notice = `Could not open ${path} in another app${why}.`;
  }
}
</script>

<div
  class="viewer"
  data-panel="Viewer"
  bind:this={root}
  onfocusin={() => (hadFocus = true)}
  onfocusout={(event) => {
    if (event.relatedTarget instanceof Node && !root?.contains(event.relatedTarget)) hadFocus = false;
  }}
>
  {#if !workspace.project}
    <p class="note">Open a project to see its files here.</p>
  {:else if !workspace.openPath}
    <StartPanel />
  {:else}
    <div class="bar">
      <span class="path" title={workspace.openPath}>{workspace.openPath}</span>
      {#if lspHint}<span class="lsp-hint" data-testid="lsp-edit-hint">{lspHint} in Edit</span>{/if}
      {#if editable}
        <button
          type="button"
          class="quiet edit"
          aria-keyshortcuts={isApple() ? "Meta+E" : "Control+E"}
          title="Edit this file ({editKeys})"
          onclick={() => toggle(workspace)}>Edit <kbd>{editKeys}</kbd></button
        >
      {/if}
      {#if loaded && "view" in loaded && loaded.view.kind !== "binary" && loaded.view.kind !== "too_large"}
        <button type="button" class="quiet" onclick={openExternal}>Open in default app</button>
      {/if}
    </div>
    <p class="notice" role="status">{notice}</p>
    <!-- A scroll area; the key handler only explains why typing does nothing. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="content" bind:this={content} tabindex="-1" onkeydown={ontyping}>
      {#if unsaved !== null}
        <p class="note unsaved" data-testid="unsaved-preview">Showing unsaved changes from the editor.</p>
      {/if}
      {#if !shown}
        <p class="note">Loading {workspace.openPath}…</p>
      {:else if "error" in shown}
        <p class="note error">{shown.error}</p>
      {:else if shown.view.kind === "markdown"}
        <MarkdownView
          text={shown.view.text}
          path={workspace.openPath}
          open={(path) => workspace.openFile(path)}
          notify={(message) => (notice = message)}
        />
      {:else if shown.view.kind === "image"}
        <ImageView src={shown.view.src} path={workspace.openPath} />
      {:else if shown.view.kind === "patch"}
        <PatchView text={shown.view.text} />
      {:else if shown.view.kind === "text"}
        <Lines items={shown.view.text.split(/\r?\n/)} label="File text" testid="text-view">
          {#snippet row(line)}
            <div class="text-line">{line}</div>
          {/snippet}
        </Lines>
      {:else if shown.view.kind === "binary"}
        <p class="note">
          This is a binary file ({shown.view.mime}, {formatSize(shown.size)}) and cannot be shown here.
          <button type="button" onclick={openExternal}>Open in default app</button>
        </p>
      {:else}
        <p class="note">
          This file is {formatSize(shown.size)}, too large to show here.
          <button type="button" onclick={openExternal}>Open in default app</button>
        </p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .viewer {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    border-bottom: 1px solid #2a2a31;
    font-size: 12px;
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #b4b4c0;
  }
  .notice {
    margin: 0;
    padding: 0 12px;
    font-size: 12px;
    color: #b4b4c0;
  }
  .notice:empty {
    display: none;
  }
  .quiet {
    background: none;
    color: #b4b4c0;
    border: 1px solid #3a3a44;
    border-radius: 3px;
    padding: 1px 8px;
    font: inherit;
    cursor: pointer;
  }
  .quiet:hover {
    color: #e6e6e6;
  }
  .quiet:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .edit {
    color: #e6e6e6;
    border-color: #4a6bb0;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    color: #9a9aa6;
  }
  .lsp-hint {
    color: #9a9aa6;
    white-space: nowrap;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }
  .note {
    padding: 12px;
    margin: 0;
    color: #8a8a96;
  }
  .error {
    color: #ff6b6b;
  }
  .text-line {
    padding: 0 12px;
  }
</style>
