<script lang="ts">
// Shows workspace.openPath in view mode: markdown, images, diffs, plain text.
// The bar above the file has Edit, so it is always in sight; typing into the
// view says that it is read-only and how to edit.
import { toggle } from "../editor/commands";
import { pathEntries } from "../editor/context-menu";
import Code from "../editor/highlight/Code.svelte";
import { highlightLines, type Segment } from "../editor/highlight/static";
import { languageFor } from "../editor/languages";
import { serverFor } from "../editor/lsp/clients";
import { unsavedText } from "../editor/open-files.svelte";
import { editorPrefs } from "../editor/prefs.svelte";
import { clearFacts, detectIndent, lineEndingOf, publishFacts } from "../editor/status.svelte";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { CoreError } from "../lib/transport";
import { languageServers } from "../lsp/servers.svelte";
import { isMenuKey, openContextMenu } from "../menu/context-menu.svelte";
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

/** The editor's text size and line height (1.65, editor/theme.ts). */
const fontSize = $derived(editorPrefs.fontSize);
const rowHeight = $derived(fontSize * 1.65);

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
const textLines = $derived(
  shown && "view" in shown && shown.view.kind === "text" ? shown.view.text.split(/\r?\n/) : [],
);
/** Highlighted `textLines`, once the worker has answered for them. */
let painted = $state.raw<{ lines: string[]; segments: Segment[][] } | null>(null);
$effect(() => {
  const lines = textLines;
  const language = workspace.openPath ? languageFor(workspace.openPath) : null;
  let live = true;
  void highlightLines(language, lines).then((segments) => {
    if (live) painted = segments && { lines, segments };
  });
  return () => {
    live = false;
  };
});
// Plain text tells the status bar its language, indentation and line endings; no cursor here.
const facts = {};
$effect(() => {
  const view = shown && "view" in shown ? shown.view : null;
  const path = workspace.openPath;
  // With two editor groups, the focused one's file speaks for the status bar.
  if (view?.kind !== "text" || !path || !workspace.focused) return;
  publishFacts(facts, {
    cursor: null,
    language: languageFor(path)?.name ?? "Plain text",
    indent: detectIndent(textLines),
    lineEnding: lineEndingOf(view.text),
  });
  return () => clearFacts(facts);
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

/** Copy, Select all and the file's path; Shift+F10 or the ContextMenu key open it too. */
function openMenu(event: MouseEvent | KeyboardEvent): void {
  const path = workspace.openPath;
  const area = content;
  if (!path || !area) return;
  const text = window.getSelection()?.toString() ?? "";
  const board = navigator.clipboard;
  openContextMenu(event, [
    board && text !== "" && { label: "Copy", keys: "Mod+C", run: () => board.writeText(text) },
    { label: "Select all", keys: "Mod+A", run: () => window.getSelection()?.selectAllChildren(area) },
    "separator",
    ...pathEntries(workspace.project?.root ?? null, path),
  ]);
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
    <div
      class="content"
      bind:this={content}
      tabindex="-1"
      onkeydown={(event) => (isMenuKey(event) ? openMenu(event) : ontyping(event))}
      oncontextmenu={openMenu}
    >
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
        <!-- Line height, gutter and padding match the editor's, so View and Edit line up. -->
        <div class="code" style:--digits={String(textLines.length).length} style:--editor-font-size="{fontSize}px">
          <Lines items={textLines} label="File text" testid="text-view" {rowHeight}>
            {#snippet row(line, i)}
              <div class="text-line" data-line={i + 1}>
                <Code text={line} segments={painted?.lines === textLines ? painted.segments[i] : null} />
              </div>
            {/snippet}
          </Lines>
        </div>
      {:else if shown.view.kind === "binary"}
        <p class="note">
          Binary file ({shown.view.mime}, {formatSize(shown.size)}): not shown.
          <button type="button" onclick={openExternal}>Open in default app</button>
        </p>
      {:else}
        <p class="note">
          Too large to show ({formatSize(shown.size)}).
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
    background: var(--s2);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px 4px 16px;
    font-size: 13px;
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    color: var(--text-2);
  }
  .notice {
    margin: 0;
    padding: 0 16px 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  .notice:empty {
    display: none;
  }
  button {
    min-height: var(--control-h-sm);
    padding: 2px 10px;
    font: inherit;
    color: var(--text-2);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s3);
  }
  .edit {
    color: var(--text);
    background: var(--s3);
  }
  .edit:hover {
    background: var(--s4);
  }
  kbd {
    font: 12px var(--mono);
    color: var(--text-2);
  }
  .lsp-hint {
    color: var(--text-2);
    white-space: nowrap;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }
  .note {
    padding: 12px 16px;
    margin: 0;
    color: var(--text-2);
  }
  .note button {
    color: var(--text);
    background: var(--s3);
  }
  .error {
    color: var(--danger);
  }
  .code {
    height: 100%;
  }
  .code :global(.viewport) {
    padding-top: 4px;
    font-size: var(--editor-font-size, 13.5px);
  }
  .text-line {
    padding-right: 16px;
  }
  /* The editor's gutters: line numbers (16 px before, 12 after), git markers (8 px), then 6 px. */
  .text-line::before {
    content: attr(data-line) / "";
    display: inline-block;
    box-sizing: content-box;
    min-width: calc(var(--digits) * 1ch);
    padding: 0 26px 0 16px;
    text-align: right;
    color: var(--text-2);
    user-select: none;
  }
  @media (forced-colors: active) {
    button {
      border-color: ButtonText;
    }
  }
</style>
