<script lang="ts" module>
import { registerContextKeys } from "../keys/context-keys";
import { isApple as apple } from "../keys/shortcut";

// This panel's own keys, for Settings > Keyboard; the panel handles them itself.
// Folding takes Ctrl+Shift off macOS, where Ctrl+Alt is AltGr (editor/folding.ts).
const fold = apple() ? "Mod+Alt" : "Ctrl+Shift";
registerContextKeys(
  [
    { keys: "Shift+F10", title: "Open the menu at the cursor (also the Menu key)" },
    { keys: "Shift+F8", title: "Go to previous problem" },
    { keys: `${fold}+[`, title: "Fold at cursor" },
    { keys: `${fold}+]`, title: "Unfold at cursor" },
  ].map((key) => ({ ...key, where: "Editor" })),
);
</script>

<script lang="ts">
// The editor for workspace.openPath (an editor group in "edit" mode). One
// view shows the active tab's buffer; buffers keep their own text, undo
// history and selection while they are not shown. In the focused group it
// reports to the status bar, the find commands and the language server.
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { untrack } from "svelte";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import { isMenuKey, openContextMenu } from "../menu/context-menu.svelte";
import { useOvra } from "../shell/context";
import type { TextSpot } from "../shell/workspace.svelte";
import type { Buffer } from "./buffers.svelte";
import { toggle } from "./commands";
import { AutoSaver, autoSaver } from "./autosave";
import Breadcrumbs from "./Breadcrumbs.svelte";
import { editorMenu } from "./context-menu";
import { answerClose, tabs } from "./open-files.svelte";
import { editorPrefs } from "./prefs.svelte";
import { hideView, showView } from "./search";
import { clearFacts, cursorOf, detectIndent, type FileFacts, publishFacts } from "./status.svelte";

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
  // Every change and cursor move passes here, whichever buffer is shown.
  const created = new EditorView({
    parent: host,
    dispatchTransactions: (transactions, target) => {
      target.update(transactions);
      if (shownView === target) track(target, transactions.some((t) => t.docChanged));
      if (shownView === target && workspace.focused) publish(target, transactions.some((t) => t.docChanged));
    },
  });
  view = created;
  return () => {
    buffer?.detach(created);
    created.destroy();
    view = null;
    clearFacts(facts);
  };
});

/** Shared by the groups: switching focus between them does not change it. */
const projectBuffers = $derived(tabs.current?.buffers);

$effect(() => {
  const buffers = projectBuffers;
  const path = workspace.openPath;
  if (!buffers || !path || !host) {
    buffer = null;
    return;
  }
  const next = buffers.open(path);
  buffer = next;
  let live = true;
  void next.loaded.then(() => {
    const target = view;
    if (!live || !target) return;
    next.attach(target);
    if (workspace.focused) {
      if (entering) target.focus();
      else focusUnlessInTabs();
    }
    entering = false;
    shown = next;
    shownView = target;
  });
  return () => {
    live = false;
    shown = null;
    shownView = null;
    next.detach(view);
    // Another tab or mode takes the editor: focus left this file.
    autoSaver.left([next]);
    // Nothing typed before the next buffer arrives may land in this one.
    view?.setState(EditorState.create());
  };
});

// The focused group's editor is the one the status bar, the find commands and the language server see.
$effect(() => {
  const target = shownView;
  const next = shown;
  const lsp = tabs.current?.lsp;
  if (!target || !next || !workspace.focused) return;
  untrack(() => {
    publish(target, true);
    showView(target);
    // Showing a file is what starts its language server (ADR 0007).
    lsp?.showing(next, target);
  });
  return () => {
    clearFacts(facts);
    hideView(target);
    lsp?.hiding(next, target);
  };
});

/** The cursor and a count of changes of the shown text, for the breadcrumbs. */
let cursor = $state(0);
let revision = $state(0);
function track(target: EditorView, docChanged: boolean): void {
  cursor = target.state.selection.main.head;
  if (docChanged) revision++;
}
$effect(() => {
  if (shownView) untrack(() => shownView && track(shownView, false));
});

/** The status bar's facts (status.svelte.ts); the indentation is read again only when the text changes. */
const facts = {};
/** The view, once it shows `shown`. */
let shownView = $state.raw<EditorView | null>(null);
let indent = "";
/** What Tab inserts, for a text with no indentation to go by. */
const ownIndent = () => (editorPrefs.insertSpaces ? `Spaces: ${editorPrefs.tabSize}` : "Tab");
/** Untracked: an effect that dispatches (word wrap, a reveal) must not come to depend on it. */
function publish(target: EditorView, docChanged: boolean): void {
  untrack(() => {
    const state = target.state;
    if (docChanged) indent = detectIndent(state.doc.iterLines(), ownIndent());
    const next: FileFacts = {
      cursor: cursorOf(state),
      language: shown?.heavy ? "Plain text (large file)" : (shown?.language?.name ?? "Plain text"),
      indent,
      // The file's own line breaks, which saving keeps.
      lineEnding: shown?.lineEnding ?? "LF",
    };
    publishFacts(facts, next);
  });
}

// The shown buffer follows wrap and indent prefs; a new text size needs a fresh measure.
$effect(() => {
  // Reads editorPrefs (wrap, tab size, spaces), so a change runs this again.
  shown?.applyPrefs();
});
$effect(() => {
  void editorPrefs.fontSize;
  view?.requestMeasure();
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

/** Whether auto save is on and may write the open buffer (not while a conflict or an error is shown). */
const autoSaving = $derived(!!buffer && editorPrefs.autoSave !== "off" && AutoSaver.ready(buffer));

/** The window lost focus: auto save on focus change writes every buffer with unsaved text. */
function leaveWindow(): void {
  const buffers = tabs.current?.buffers;
  // One editor speaks for all buffers.
  if (!buffers || !workspace.focused) return;
  autoSaver.left(buffers.unsavedPaths().flatMap((path) => buffers.get(path) ?? []));
}

async function save(overwrite = false): Promise<void> {
  await buffer?.save(overwrite);
  view?.focus();
}

async function revert(): Promise<void> {
  await buffer?.revert();
  view?.focus();
}

/** The context menu: a right-click first moves the cursor there unless it hits the selection; a key opens it at the cursor. */
function openMenu(event: MouseEvent | KeyboardEvent): void {
  const target = view;
  const path = shown?.path;
  // The find and go-to-line fields keep the native menu.
  if (!target || !path || (event.target instanceof Element && event.target.closest(".cm-panels"))) return;
  const root = workspace.project?.root ?? null;
  if (event instanceof MouseEvent && event.button === 2) {
    const pos = target.posAtCoords({ x: event.clientX, y: event.clientY });
    const { from, to } = target.state.selection.main;
    if (pos !== null && (pos < from || pos > to)) target.dispatch({ selection: { anchor: pos } });
    openContextMenu(event, editorMenu(target, root, path, !workspace.other));
    return;
  }
  event.preventDefault();
  const at = target.coordsAtPos(target.state.selection.main.head);
  const entries = editorMenu(target, root, path, !workspace.other);
  openContextMenu({ x: at?.left ?? 0, y: at?.bottom ?? 0, returnFocus: target.contentDOM }, entries);
}

function discard(path: string): void {
  tabs.closing = path;
  void answerClose(workspace, "discard");
}
</script>

<svelte:window onblur={leaveWindow} />

<div class="editor" data-panel="Editor">
  {#if buffer}
    <div class="bar">
      <Breadcrumbs
        path={buffer.path}
        view={shown === buffer ? shownView : null}
        {cursor}
        {revision}
        outline={!buffer.heavy}
      >
        {#if buffer.dirty}
          {@const label = autoSaving ? "Unsaved changes, saved automatically" : "Unsaved changes"}
          <span class="dirty" class:auto={autoSaving} title={label} data-testid="editor-dirty"
            >●<span class="visually-hidden">{label}</span></span
          >
        {/if}
      </Breadcrumbs>
      <span class="language" data-testid="editor-language"
        >{buffer.heavy ? "Plain text (large file)" : (buffer.language?.name ?? "Plain text")}</span
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
      <p class="message">Binary file: not editable.</p>
    {:else if buffer.status === "too_large"}
      <p class="message">Too large to edit.</p>
    {/if}
  {:else}
    <p class="message">No file open.</p>
  {/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="host"
    class:hidden={buffer?.status !== "ready"}
    style:--editor-font-size="{editorPrefs.fontSize}px"
    bind:this={host}
    oncontextmenu={openMenu}
    onkeydown={(event) => isMenuKey(event) && openMenu(event)}
  ></div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--s2);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px 4px 16px;
    font-size: 13px;
    color: var(--text-2);
  }
  .dirty {
    color: var(--accent);
  }
  /* Auto save will take care of it: no call to act. */
  .dirty.auto {
    color: var(--text-2);
  }
  .language {
    margin-left: auto;
    white-space: nowrap;
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
  button:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  /* Save is filled only while there is something to save. */
  .primary:not(:disabled),
  .primary:hover:not(:disabled) {
    color: var(--accent-ink);
    background: var(--accent);
    font-weight: 500;
  }
  button:disabled {
    color: var(--text-2);
    cursor: default;
  }
  kbd {
    font: 12px var(--mono);
    color: var(--text-2);
  }
  .notice {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0 12px 6px;
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--warn-soft);
    color: var(--text);
  }
  .notice button {
    color: var(--text);
    background: var(--s3);
  }
  .message {
    padding: 16px;
    color: var(--text-2);
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
  @media (forced-colors: active) {
    button {
      border-color: ButtonText;
    }
  }
</style>
