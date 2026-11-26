<script lang="ts" module>
import { registerContextKeys } from "../keys/context-keys";

// This panel's own keys, for Settings > Keyboard; the panel handles them itself.
registerContextKeys(
  [
    { keys: "Shift+F10", title: "Open the tab's menu (also the Menu key)" },
    { keys: "Left/Right", title: "Previous or next tab" },
    { keys: "Delete", title: "Close the tab" },
  ].map((key) => ({ ...key, where: "Tabs" })),
);
</script>

<script lang="ts">
// Tabs for the open files of the project, above the main area. One Tab stop
// for the strip; arrows, Home and End move between tabs, Delete closes one.
// Closing a tab with unsaved text asks Save / Don't save / Cancel in a
// modal dialog; Cancel puts focus back where it was. Edit / View lives in
// the file's own bar, so it never scrolls away with the tabs. The strip fades
// at an edge that has more tabs beyond it.
// The preview tab (a file opened with a single click) has an italic name;
// a double click keeps it. Pinned tabs come first, show only their name and
// trade the close button for an unpin one; others show a pin button on hover.
// A git diff shown in the main area is the last tab, selected while it shows.
// A tab's context menu (right click, Shift+F10, the Menu key): the file's
// path and places first, then pinning and the other editor group, then the
// ways to close tabs. With two editor groups each has its own strip; a tab
// can be dragged onto the other group (docs/specs/split-editor.md), or
// within its strip to reorder, with a line where it would land; Escape
// cancels the drag.
import { tick } from "svelte";
import type { ProjectId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { isMenuKey, openContextMenu } from "../menu/context-menu.svelte";
import { useOvra } from "../shell/context";
import FileIcon from "../shell/file-icons/FileIcon.svelte";
import Icon from "../shell/Icon.svelte";
import type { GroupIndex } from "../shell/workspace.svelte";
import { revealLabel } from "../tree/ops";
import { pathEntries } from "./context-menu";
import { tabLabels } from "./labels";
import {
  answerClose,
  closeOtherTabs,
  closeSavedTabs,
  closeTabsToTheRight,
  dragging,
  moveTab,
  requestClose,
  show,
  splitRight,
  tabs,
} from "./open-files.svelte";

let { group = 0 }: { group?: GroupIndex } = $props();

const { transport, workspace } = useOvra();
/** This group's tabs. */
const own = $derived(tabs.groups[group] ?? null);
/** Commands act on the focused group: an action on one of these tabs focuses it first. */
const focusHere = () => workspace.focusGroup(group);

let strip = $state<HTMLDivElement>();
let dialog = $state<HTMLDialogElement>();
/** What had focus before the close question, for Cancel. */
let beforeQuestion: HTMLElement | null = null;
/** Whether more tabs are hidden past the left or right edge of the strip. */
let more = $state({ start: false, end: false });

function measure(): void {
  if (!strip) return;
  const { scrollLeft, scrollWidth, clientWidth } = strip;
  more = { start: scrollLeft > 1, end: scrollLeft + clientWidth < scrollWidth - 1 };
}

$effect(() => {
  const target = strip;
  if (!target || typeof ResizeObserver === "undefined") return;
  const observer = new ResizeObserver(measure);
  observer.observe(target);
  return () => observer.disconnect();
});

$effect(() => {
  void own?.files.length;
  void tick().then(measure);
});

/** Where a tab dragged within this strip would land: before the tab at this index; null while none is. */
let dropAt = $state<number | null>(null);

/** A tab of this strip is dragged over it: it would land before the first tab whose middle is right of the pointer. */
function ondragover(event: DragEvent): void {
  if (dragging.tab?.group !== group) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  const tabs = [...(strip?.querySelectorAll<HTMLElement>('[role="tab"][data-path]') ?? [])];
  const at = tabs.findIndex((tab) => {
    const box = (tab.parentElement ?? tab).getBoundingClientRect();
    return event.clientX < box.left + box.width / 2;
  });
  dropAt = at < 0 ? tabs.length : at;
}

function ondropHere(event: DragEvent): void {
  const tab = dragging.tab;
  const at = dropAt;
  dropAt = null;
  if (tab?.group !== group || at === null) return;
  event.preventDefault();
  dragging.tab = null;
  own?.moveTo(tab.path, at);
}

function onstripdragleave(event: DragEvent): void {
  if (!(event.relatedTarget instanceof Node && strip?.contains(event.relatedTarget))) dropAt = null;
}

function label(path: string): string {
  const entry = labels.get(path);
  return entry?.hint ? `${entry.name} - ${entry.hint}` : (entry?.name ?? path);
}

function togglePin(path: string): void {
  own?.togglePin(path);
  void focusTab(path);
}

/**
 * Tab labels: the file name, plus for names shared by several tabs the
 * shortest trailing part of the folder that tells them apart (`src` in
 * `pkg-003/src`), shown muted. The full path is in the tooltip.
 */
let labels = $derived(tabLabels(own?.files ?? []));

/** A git diff of this project shown in the main area: a tab of its own, after the files, selected while it shows. */
const diff = $derived(
  workspace.focused && workspace.gitDiff?.project === workspace.project?.id ? workspace.gitDiff : null,
);
const diffName = $derived(diff ? diff.path.slice(diff.path.lastIndexOf("/") + 1) : "");

function closeDiff(): void {
  // The diff view puts focus back where the diff was opened from.
  workspace.gitDiff = null;
}

function ondiffkeydown(event: KeyboardEvent): void {
  const files = own?.files ?? [];
  const target = { ArrowRight: files[0], Home: files[0], ArrowLeft: files.at(-1) }[event.key];
  if (target) {
    event.preventDefault();
    select(target);
  } else if (event.key === "Delete") {
    event.preventDefault();
    closeDiff();
  }
}

async function focusTab(path: string | null): Promise<void> {
  await tick();
  const tab = [...(strip?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].find((t) => t.dataset.path === path);
  tab?.focus();
  tab?.parentElement?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/** After a tab closed: focus the next tab, or the main area when none is left. */
function focusAfterClose(next: string | null | undefined): void {
  // With no tab left, closeTab() moves focus to the tree.
  if (next) void focusTab(next);
}

function close(path: string): void {
  const files = own?.files ?? [];
  const index = files.indexOf(path);
  const next = path === own?.active ? (files[index + 1] ?? files[index - 1]) : own?.active;
  focusHere();
  requestClose(workspace, path);
  if (!tabs.closing) focusAfterClose(next);
}

function select(path: string): void {
  show(workspace, path);
  void focusTab(path);
}

async function revealInOs(project: ProjectId, path: string): Promise<void> {
  try {
    await transport.call({ method: "open.reveal", params: { project, path } });
  } catch (err) {
    tabs.notice = err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
  }
}

/** The menu of the tab for `path`; items that do not apply are left out (docs/design/components.md). */
function menu(at: MouseEvent | KeyboardEvent, path: string): void {
  const current = own;
  if (!current) return;
  focusHere();
  const files = current.files;
  const other: GroupIndex = group === 0 ? 1 : 0;
  const unpinned = (p: string) => !current.isPinned(p);
  const after = files.slice(files.indexOf(path) + 1);
  const project = workspace.project;
  openContextMenu(at, [
    ...pathEntries(project?.root ?? null, path, async () => {
      show(workspace, path);
      await tick();
    }),
    project && { label: revealLabel(), run: () => revealInOs(project.id, path) },
    "separator",
    current.preview === path && { label: "Keep tab", run: () => current.makePermanent(path) },
    { label: current.isPinned(path) ? "Unpin tab" : "Pin tab", run: () => togglePin(path) },
    !workspace.other && {
      label: "Split right",
      keys: "Mod+\\",
      run: () => {
        show(workspace, path);
        splitRight(workspace);
      },
    },
    (workspace.other || files.length > 1) && {
      label: "Move to other group",
      run: () => moveTab(workspace, path, other),
    },
    "separator",
    { label: "Close", keys: path === current.active ? "Mod+W" : undefined, run: () => close(path) },
    files.some((p) => p !== path && unpinned(p)) && {
      label: "Close others",
      run: () => closeOtherTabs(workspace, path),
    },
    after.some(unpinned) && { label: "Close to the right", run: () => closeTabsToTheRight(workspace, path) },
    files.some((p) => unpinned(p) && !current.buffers.isDirty(p)) && {
      label: "Close saved",
      run: () => closeSavedTabs(workspace),
    },
    files.some(unpinned) && { label: "Close all", run: () => closeOtherTabs(workspace, null) },
  ]);
}

function onkeydown(event: KeyboardEvent, path: string): void {
  if (isMenuKey(event)) {
    menu(event, path);
    return;
  }
  const files = own?.files ?? [];
  const index = files.indexOf(path);
  const target = {
    ArrowRight: files[(index + 1) % files.length],
    ArrowLeft: files[(index - 1 + files.length) % files.length],
    Home: files[0],
    End: files.at(-1),
  }[event.key];
  if (target) {
    event.preventDefault();
    select(target);
  } else if (event.key === "Delete") {
    event.preventDefault();
    close(path);
  }
}

async function answer(choice: "save" | "discard" | "cancel"): Promise<void> {
  const path = tabs.closing;
  const back = beforeQuestion;
  beforeQuestion = null;
  await answerClose(workspace, choice);
  if (choice === "cancel" && back?.isConnected && !strip?.contains(back)) {
    // Never mind: back to where the person was, usually the text.
    await tick();
    back.focus();
  } else if (choice === "cancel" || own?.files.includes(path ?? "")) void focusTab(path);
  else focusAfterClose(own?.active);
}

// A real modal: the rest of the page is inert and Tab stays inside.
$effect(() => {
  if (tabs.closing && dialog && !dialog.open) {
    beforeQuestion = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    dialog.querySelector<HTMLButtonElement>("button")?.focus();
  }
});

// Saved some other way (Ctrl+S) while the question is open: nothing left to ask.
$effect(() => {
  const path = tabs.closing;
  if (path && workspace.focused && !tabs.saving && own && !own.buffers.isDirty(path)) void answer("discard");
});

function ondialogkeydown(event: KeyboardEvent): void {
  if (event.key === "Tab" && dialog) {
    // Keep Tab inside the dialog, wrapping at both ends.
    const buttons = [...dialog.querySelectorAll<HTMLButtonElement>("button")];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = buttons[(index + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length];
    event.preventDefault();
    next?.focus();
  } else if ((event.ctrlKey || event.metaKey) && event.code === "KeyS") {
    event.preventDefault();
    event.stopPropagation();
    void answer("save");
  }
}

$effect(() => {
  const active = own?.active;
  if (active) void focusTab(null).then(() => scrollToActive());
});

function scrollToActive(): void {
  strip
    ?.querySelector('[aria-selected="true"]')
    ?.parentElement?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
</script>

{#if (own && own.files.length > 0) || diff}
  {@const files = own?.files ?? []}
  {@const selected = diff ? null : (own?.active ?? null)}
  <!-- The tabs take focus, not the strip; it only takes drops. -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="tabs"
    class:more-start={more.start}
    class:more-end={more.end}
    role="tablist"
    aria-label="Open files"
    data-testid="tabs"
    bind:this={strip}
    onscroll={measure}
    {ondragover}
    ondragleave={onstripdragleave}
    ondrop={ondropHere}
  >
    {#each files as path, index (path)}
      {@const active = path === selected}
      {@const dirty = own?.buffers.isDirty(path) ?? false}
      {@const pinned = own?.isPinned(path) ?? false}
      {@const preview = own?.preview === path}
      <div
        class="tab"
        class:active
        class:pinned
        class:preview
        class:drop-before={dropAt === index}
        class:drop-after={dropAt === files.length && index === files.length - 1}
        title={preview ? `${path} (preview)` : path}
      >
        <button
          type="button"
          role="tab"
          data-path={path}
          aria-selected={active}
          aria-controls="ovra-tabpanel"
          aria-label={`${label(path)}${pinned ? ", pinned" : ""}${preview ? ", preview" : ""}${dirty ? ", unsaved changes" : ""}`}
          tabindex={active || (!diff && !selected && path === files[0]) ? 0 : -1}
          onclick={() => select(path)}
          oncontextmenu={(event) => menu(event, path)}
          ondblclick={() => own?.makePermanent(path)}
          draggable="true"
          ondragstart={(event) => {
            dragging.tab = { group, path };
            if (event.dataTransfer) {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", path);
            }
          }}
          ondragend={() => {
            dragging.tab = null;
            dropAt = null;
          }}
          onkeydown={(event) => onkeydown(event, path)}
        >
          <FileIcon name={path.slice(path.lastIndexOf("/") + 1)} />
          <span class="name">{labels.get(path)?.name}</span>
          {#if labels.get(path)?.hint && !pinned}<span class="hint">{labels.get(path)?.hint}</span>{/if}
        </button>
        {#if pinned}
          <button
            type="button"
            class="close"
            class:dirty
            tabindex="-1"
            aria-label={`Unpin ${label(path)}`}
            onclick={() => togglePin(path)}
            oncontextmenu={(event) => menu(event, path)}
            ><span class="x"><Icon name="pin" small /></span>{#if dirty}<span class="dot"></span
              >{/if}</button
          >
        {:else}
          <button
            type="button"
            class="pin"
            tabindex="-1"
            aria-label={`Pin ${label(path)}`}
            oncontextmenu={(event) => menu(event, path)}
            onclick={() => togglePin(path)}><Icon name="pin" small /></button
          >
          <button
          type="button"
          class="close"
          class:dirty
          tabindex="-1"
          aria-label={dirty ? `Close ${label(path)} (unsaved changes)` : `Close ${label(path)}`}
          onclick={() => close(path)}
          oncontextmenu={(event) => menu(event, path)}
          ><span class="x"><Icon name="close" small /></span>{#if dirty}<span class="dot"></span
            >{/if}</button
          >
        {/if}
      </div>
    {/each}
    {#if diff}
      {@const side = diff.staged ? "staged diff" : "diff"}
      <div class="tab active" title={`${diff.path} (${side})`}>
        <button
          type="button"
          role="tab"
          data-diff-tab
          aria-selected="true"
          aria-controls="ovra-tabpanel"
          aria-label={`${diffName} (${side})`}
          tabindex="0"
          onkeydown={ondiffkeydown}
        >
          <FileIcon name={diffName} />
          <span class="name">{diffName}</span>
          <span class="hint">{side}</span>
        </button>
        <button
          type="button"
          class="close"
          tabindex="-1"
          aria-label={`Close ${diffName} (${side})`}
          onclick={closeDiff}><span class="x"><Icon name="close" small /></span></button
        >
      </div>
    {/if}
  </div>
{/if}

<!-- The notice and the close question belong to the focused group, where commands act. -->
{#if tabs.notice && workspace.focused}<p class="notice" role="alert">{tabs.notice}</p>{/if}

{#if tabs.closing && workspace.focused}
  {@const closing = tabs.closing}
  <dialog
    class="confirm"
    aria-modal="true"
    aria-labelledby="ovra-close-question"
    bind:this={dialog}
    oncancel={(event) => {
      event.preventDefault();
      void answer("cancel");
    }}
    onkeydown={ondialogkeydown}
  >
    <p id="ovra-close-question">Save changes to {closing} before closing?</p>
    <div class="answers">
      <button type="button" disabled={tabs.saving} onclick={() => answer("save")}
        >{tabs.saving ? "Saving…" : "Save"}</button
      >
      <button type="button" disabled={tabs.saving} onclick={() => answer("discard")}>Don't save</button>
      <button type="button" disabled={tabs.saving} onclick={() => answer("cancel")}>Cancel</button>
    </div>
  </dialog>
{/if}

<style>
  /* The strip sits on --s1; the selected tab takes the editor's --s2 and joins it. */
  .tabs {
    display: flex;
    flex-shrink: 0;
    align-items: flex-end;
    gap: 2px;
    overflow-x: auto;
    padding: 4px 8px 0;
    background: var(--s1);
    user-select: none;
    scrollbar-width: thin;
    --fade-start: 0px;
    --fade-end: 0px;
  }
  .more-start,
  .more-end {
    mask-image: linear-gradient(
      to right,
      transparent,
      black var(--fade-start),
      black calc(100% - var(--fade-end)),
      transparent
    );
  }
  .more-start {
    --fade-start: 32px;
  }
  .more-end {
    --fade-end: 32px;
  }
  /* Tone, not lines: no underline, no borders. */
  .tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    height: 32px;
    box-sizing: border-box;
    border-radius: var(--r-md) var(--r-md) 0 0;
    color: var(--text-2);
  }
  /* Where a dragged tab would land: a 2 px line in the gap before or after a tab. */
  .tab.drop-before,
  .tab.drop-after {
    position: relative;
  }
  .tab.drop-before::before,
  .tab.drop-after::after {
    content: "";
    position: absolute;
    top: 4px;
    bottom: 4px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
    pointer-events: none;
  }
  .tab.drop-before::before {
    left: -2px;
  }
  .tab.drop-after::after {
    right: -2px;
  }
  @media (forced-colors: active) {
    .tab.drop-before::before,
    .tab.drop-after::after {
      background: Highlight;
    }
  }
  .tab.preview .name {
    font-style: italic;
  }
  .tab.pinned [role="tab"] {
    max-width: 120px;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--text);
    font-weight: 500;
    background: var(--s2);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: default;
  }
  [role="tab"] {
    display: flex;
    gap: 6px;
    max-width: 240px;
    padding: 0 2px 0 10px;
    align-self: stretch;
    align-items: center;
    white-space: nowrap;
  }
  .name,
  .hint {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    flex-shrink: 0;
    max-width: 100%;
  }
  .hint {
    min-width: 0;
    font-size: 13px;
    font-weight: 400;
    color: var(--text-2);
  }
  .close,
  .pin {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    opacity: 0;
  }
  /* Pin on demand: only on the pointed-at or keyboard-focused tab, before its close button. */
  .pin {
    margin-right: 0;
  }
  .tab:hover .pin,
  .tab:has(:focus-visible) .pin,
  .tab.pinned .close {
    opacity: 1;
  }
  /* Chrome on demand: the close button shows on the active, pointed-at or keyboard-focused tab. */
  .tab.active .close,
  .tab:hover .close,
  .tab:has(:focus-visible) .close,
  .close.dirty {
    opacity: 1;
  }
  .close > span {
    display: grid;
  }
  /* Unsaved: a 6 px dot in the close slot, which turns into the close
     button while the tab is pointed at or has keyboard focus. */
  .close > .dot {
    width: 6px;
    height: 6px;
    border-radius: var(--r-pill);
    background: var(--text-2);
  }
  .close.dirty .x,
  .tab:hover .close.dirty .dot,
  .tab:has(:focus-visible) .close.dirty .dot {
    display: none;
  }
  .tab:hover .close.dirty .x,
  .tab:has(:focus-visible) .close.dirty .x {
    display: grid;
  }
  @media (forced-colors: active) {
    .close > .dot {
      background: CanvasText;
    }
  }
  .close:hover,
  .pin:hover {
    background: var(--s4);
    color: var(--text);
  }
  @media (forced-colors: active) {
    .tab.active {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
    .close,
    .pin {
      opacity: 1;
    }
  }
  .notice {
    margin: 0;
    padding: 8px 16px;
    background: var(--accent-soft);
    color: var(--text);
  }
  .confirm {
    padding: 18px 20px;
    border: 0;
    border-radius: var(--radius);
    background: var(--s4);
    box-shadow: var(--card-shadow);
    color: var(--text);
  }
  .confirm::backdrop {
    background: var(--scrim);
  }
  .confirm p {
    margin: 0 0 14px;
    overflow-wrap: anywhere;
  }
  .answers {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .confirm button {
    padding: 4px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-2);
  }
  .confirm button:hover {
    background: var(--s3);
    color: var(--text);
  }
  .confirm button:first-child {
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 500;
  }
  @media (forced-colors: active) {
    .confirm {
      border: 1px solid CanvasText;
    }
  }
</style>
