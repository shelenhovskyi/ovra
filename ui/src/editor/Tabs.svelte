<script lang="ts">
// Tabs for the open files of the project, above the main area. One Tab stop
// for the strip; arrows, Home and End move between tabs, Delete closes one.
// Closing a tab with unsaved text asks Save / Don't save / Cancel in a
// modal dialog; Cancel puts focus back where it was. Edit / View lives in
// the file's own bar, so it never scrolls away with the tabs. The strip fades
// at an edge that has more tabs beyond it.
// A git diff shown in the main area is the last tab, selected while it shows.
import { tick } from "svelte";
import { useOvra } from "../shell/context";
import { tabLabels } from "./labels";
import { answerClose, requestClose, show, tabs } from "./open-files.svelte";

const { workspace } = useOvra();

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
  void tabs.current?.files.length;
  void tick().then(measure);
});

function label(path: string): string {
  const entry = labels.get(path);
  return entry?.hint ? `${entry.name} - ${entry.hint}` : (entry?.name ?? path);
}

/**
 * Tab labels: the file name, plus for names shared by several tabs the
 * shortest trailing part of the folder that tells them apart (`src` in
 * `pkg-003/src`), shown muted. The full path is in the tooltip.
 */
let labels = $derived(tabLabels(tabs.current?.files ?? []));

/** A git diff of this project shown in the main area: a tab of its own, after the files, selected while it shows. */
const diff = $derived(workspace.gitDiff?.project === workspace.project?.id ? workspace.gitDiff : null);
const diffName = $derived(diff ? diff.path.slice(diff.path.lastIndexOf("/") + 1) : "");

function closeDiff(): void {
  // The diff view puts focus back where the diff was opened from.
  workspace.gitDiff = null;
}

function ondiffkeydown(event: KeyboardEvent): void {
  const files = tabs.current?.files ?? [];
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
  const files = tabs.current?.files ?? [];
  const index = files.indexOf(path);
  const next = path === tabs.current?.active ? (files[index + 1] ?? files[index - 1]) : tabs.current?.active;
  requestClose(workspace, path);
  if (!tabs.closing) focusAfterClose(next);
}

function select(path: string): void {
  show(workspace, path);
  void focusTab(path);
}

function onkeydown(event: KeyboardEvent, path: string): void {
  const files = tabs.current?.files ?? [];
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
  } else if (choice === "cancel" || tabs.current?.files.includes(path ?? "")) void focusTab(path);
  else focusAfterClose(tabs.current?.active);
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
  if (path && !tabs.saving && tabs.current && !tabs.current.buffers.isDirty(path)) void answer("discard");
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
  const active = tabs.current?.active;
  if (active) void focusTab(null).then(() => scrollToActive());
});

function scrollToActive(): void {
  strip
    ?.querySelector('[aria-selected="true"]')
    ?.parentElement?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
</script>

{#if (tabs.current && tabs.current.files.length > 0) || diff}
  {@const files = tabs.current?.files ?? []}
  {@const selected = diff ? null : (tabs.current?.active ?? null)}
  <div
    class="tabs"
    class:more-start={more.start}
    class:more-end={more.end}
    role="tablist"
    aria-label="Open files"
    data-testid="tabs"
    bind:this={strip}
    onscroll={measure}
  >
    {#each files as path (path)}
      {@const active = path === selected}
      {@const dirty = tabs.current?.buffers.isDirty(path) ?? false}
      <div class="tab" class:active title={path}>
        <button
          type="button"
          role="tab"
          data-path={path}
          aria-selected={active}
          aria-controls="ovra-tabpanel"
          aria-label={`${label(path)}${dirty ? ", unsaved changes" : ""}`}
          tabindex={active || (!diff && !selected && path === files[0]) ? 0 : -1}
          onclick={() => select(path)}
          onkeydown={(event) => onkeydown(event, path)}
        >
          <span class="name">{labels.get(path)?.name}</span>
          {#if labels.get(path)?.hint}<span class="hint">{labels.get(path)?.hint}</span>{/if}
        </button>
        <button
          type="button"
          class="close"
          class:dirty
          tabindex="-1"
          aria-label={dirty ? `Close ${label(path)} (unsaved changes)` : `Close ${label(path)}`}
          onclick={() => close(path)}
        ></button>
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
          <span class="name">{diffName}</span>
          <span class="hint">{side}</span>
        </button>
        <button
          type="button"
          class="close"
          tabindex="-1"
          aria-label={`Close ${diffName} (${side})`}
          onclick={closeDiff}
        ></button>
      </div>
    {/if}
  </div>
{/if}

{#if tabs.notice}<p class="notice" role="alert">{tabs.notice}</p>{/if}

{#if tabs.closing}
  {@const closing = tabs.closing}
  <dialog
    class="confirm"
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
  .tabs {
    display: flex;
    flex-shrink: 0;
    gap: 2px;
    overflow-x: auto;
    padding: 6px 10px 0;
    background: var(--s2);
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
  /* No boxes: the active tab is told by its text and a slim accent underline. */
  .tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    border-bottom: 2px solid transparent;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    color: var(--text-2);
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--text);
    font-weight: 500;
    border-bottom-color: var(--accent);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: pointer;
  }
  [role="tab"] {
    display: flex;
    align-items: baseline;
    gap: 6px;
    max-width: 240px;
    padding: 6px 2px 6px 10px;
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
  .close {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    opacity: 0;
  }
  /* Chrome on demand: the close button shows on the active or pointed-at tab. */
  .tab.active .close,
  .tab:hover .close,
  .close:focus-visible,
  .close.dirty {
    opacity: 1;
  }
  .close::before {
    content: "×";
    font-size: 16px;
    line-height: 1;
  }
  .close.dirty::before {
    content: "●";
    font-size: 10px;
    color: var(--accent);
  }
  .close:hover {
    background: var(--s3);
    color: var(--text);
  }
  .close.dirty:hover::before {
    content: "×";
    font-size: 16px;
    color: inherit;
  }
  @media (forced-colors: active) {
    .tab.active {
      border-bottom-color: Highlight;
    }
    .close {
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
