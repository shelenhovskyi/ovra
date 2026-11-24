<script lang="ts">
// Tabs for the open files of the project, above the main area. One Tab stop
// for the strip; arrows, Home and End move between tabs, Delete closes one.
// Closing a tab with unsaved text asks Save / Don't save / Cancel in a
// modal dialog. The Edit / View button switches the open file's mode.
import { tick } from "svelte";
import { useOvra } from "../shell/context";
import { toggle } from "./commands";
import { answerClose, focusMain, requestClose, show, tabs } from "./open-files.svelte";

const { workspace } = useOvra();

let strip = $state<HTMLDivElement>();
let dialog = $state<HTMLDialogElement>();

function name(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** File names, with the parent folder added where two tabs share a name. */
let labels = $derived.by(() => {
  const files = tabs.current?.files ?? [];
  const counts = new Map<string, number>();
  for (const path of files) counts.set(name(path), (counts.get(name(path)) ?? 0) + 1);
  return new Map(
    files.map((path) => {
      const base = name(path);
      const dir = path.slice(0, Math.max(0, path.lastIndexOf("/")));
      return [path, (counts.get(base) ?? 0) > 1 && dir ? `${base} - ${dir}` : base];
    }),
  );
});

async function focusTab(path: string | null): Promise<void> {
  await tick();
  const tab = [...(strip?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].find((t) => t.dataset.path === path);
  tab?.focus();
  tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/** After a tab closed: focus the next tab, or the main area when none is left. */
function focusAfterClose(next: string | null | undefined): void {
  if (next) void focusTab(next);
  else void focusMain();
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
  await answerClose(workspace, choice);
  if (choice === "cancel" || tabs.current?.files.includes(path ?? "")) void focusTab(path);
  else focusAfterClose(tabs.current?.active);
}

// A real modal: the rest of the page is inert and Tab stays inside.
$effect(() => {
  if (tabs.closing && dialog && !dialog.open) {
    dialog.showModal();
    dialog.querySelector<HTMLButtonElement>("button")?.focus();
  }
});

// Saved some other way (Ctrl+S) while the question is open: nothing left to ask.
$effect(() => {
  const path = tabs.closing;
  if (path && tabs.current && !tabs.current.buffers.isDirty(path)) void answer("discard");
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
  strip?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
</script>

{#if tabs.current && tabs.current.files.length > 0}
  {@const current = tabs.current}
  <div class="tabs" role="tablist" aria-label="Open files" data-testid="tabs" bind:this={strip}>
    {#each current.files as path (path)}
      {@const active = path === current.active}
      {@const dirty = current.buffers.isDirty(path)}
      <div class="tab" class:active title={path}>
        <button
          type="button"
          role="tab"
          data-path={path}
          aria-selected={active}
          aria-controls="ovra-tabpanel"
          aria-label={dirty ? `${labels.get(path)}, unsaved changes` : labels.get(path)}
          tabindex={active || (!current.active && path === current.files[0]) ? 0 : -1}
          onclick={() => select(path)}
          onkeydown={(event) => onkeydown(event, path)}
        >
          {labels.get(path)}
        </button>
        <button
          type="button"
          class="close"
          class:dirty
          tabindex="-1"
          aria-label={dirty ? `Close ${labels.get(path)} (unsaved changes)` : `Close ${labels.get(path)}`}
          onclick={() => close(path)}
        ></button>
      </div>
    {/each}
    {#if workspace.openPath}
      <button type="button" class="mode" onclick={() => toggle(workspace)}>
        {workspace.mode === "edit" ? "View" : "Edit"}
      </button>
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
      <button type="button" onclick={() => answer("save")}>Save</button>
      <button type="button" onclick={() => answer("discard")}>Don't save</button>
      <button type="button" onclick={() => answer("cancel")}>Cancel</button>
    </div>
  </dialog>
{/if}

<style>
  .tabs {
    display: flex;
    flex-shrink: 0;
    overflow-x: auto;
    border-bottom: 1px solid #2a2a31;
    background: #121215;
    scrollbar-width: thin;
  }
  .tab {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    border-right: 1px solid #2a2a31;
    border-bottom: 2px solid transparent;
    color: #9a9aa6;
  }
  .tab.active {
    background: #16161a;
    color: #e6e6e6;
    border-bottom-color: #82aaff;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    cursor: pointer;
  }
  [role="tab"] {
    padding: 6px 4px 6px 12px;
  }
  .close {
    width: 20px;
    height: 20px;
    margin-right: 6px;
    border-radius: 3px;
  }
  .close::before {
    content: "×";
  }
  .close.dirty::before {
    content: "●";
    font-size: 10px;
  }
  .close:hover {
    background: #2a2a31;
  }
  .close.dirty:hover::before {
    content: "×";
    font-size: inherit;
  }
  @media (forced-colors: active) {
    .tab.active {
      border-bottom-color: Highlight;
    }
  }
  .notice {
    margin: 0;
    padding: 6px 12px;
    background: #2a2a3a;
    color: #e6e6e6;
  }
  .mode {
    margin-left: auto;
    flex-shrink: 0;
    padding: 4px 12px;
    color: #82aaff;
  }
  .mode:hover {
    background: #2a2a31;
  }
  .confirm {
    padding: 16px;
    border: 1px solid #4a4a5a;
    border-radius: 6px;
    background: #1d1d23;
    color: #e6e6e6;
  }
  .confirm::backdrop {
    background: rgb(0 0 0 / 40%);
  }
  .confirm p {
    margin: 0 0 12px;
    overflow-wrap: anywhere;
  }
  .answers {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .confirm button {
    padding: 2px 8px;
    border: 1px solid #4a4a5a;
    border-radius: 3px;
  }
</style>
