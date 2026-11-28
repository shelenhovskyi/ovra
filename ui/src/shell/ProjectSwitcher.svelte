<script lang="ts">
// The header's project name, a menu button: the other recent projects,
// "Open a folder…" and "Close project". Keys as in the More menu: arrows,
// Home, End and a first letter move; Escape closes and gives focus back.
// Without the system's folder dialog (a browser tab), "Open a folder…" turns
// the menu into a path field; the current project stays open until the typed
// folder opens, and Escape or Cancel goes back to it.
import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { Project } from "../lib/protocol";
import { FolderOpener } from "../tree/folder-opener.svelte";
import { useOvra } from "./context";
import Icon from "./Icon.svelte";
import { menuTarget } from "./menu";
import { folderDialogCommand, openAnotherFolder } from "./open-folder";

/** Recent projects shown at most. */
const RECENT = 8;

let { project }: { project: Project } = $props();

const { transport, workspace } = useOvra();
const opener = new FolderOpener(transport, (opened) => {
  workspace.openPath = null;
  workspace.mode = "view";
  workspace.project = opened;
});

let open = $state(false);
let recent = $state<Project[]>([]);
let folderKeys = $state<string | undefined>();
let button = $state<HTMLButtonElement>();
let menu = $state<HTMLElement>();
let place = $state("");
/** The menu shows the folder path field instead of its items. */
let typing = $state(false);
let path = $state("");
let field = $state<HTMLInputElement>();

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
}

async function show(focus: "first" | "last"): Promise<void> {
  const at = button?.getBoundingClientRect();
  place = at ? `top: ${at.bottom + 4}px; left: ${at.left}px;` : "";
  opener.error = null;
  typing = false;
  const keys = folderDialogCommand()?.keys;
  folderKeys = keys ? liveKeys(keys, inBrowserTab()) : undefined;
  try {
    const all = (await transport.call({ method: "projects.list" })).data;
    recent = all.filter((p) => p.id !== project.id).slice(0, RECENT);
  } catch {
    // The menu still opens a folder or closes the project without the list.
    recent = [];
  }
  open = true;
  await tick();
  const all = items();
  (focus === "first" ? all[0] : all[all.length - 1])?.focus();
}

function close(refocus: boolean): void {
  open = false;
  typing = false;
  path = "";
  if (refocus) button?.focus();
}

async function switchTo(other: Project): Promise<void> {
  await opener.openKnown(other);
  // A failure stays in the menu, next to the project that did not open.
  if (!opener.error) close(true);
}

async function openFolder(): Promise<void> {
  if (folderDialogCommand()) {
    close(true);
    void openAnotherFolder(workspace);
    return;
  }
  opener.error = null;
  typing = true;
  await tick();
  field?.focus();
}

async function openTyped(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  // A refused path stays in the field, with the reason under it.
  if (await opener.open(path.trim())) close(true);
}

function onFormKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  close(true);
}

function closeProject(): void {
  close(false);
  void listCommands()
    .find((c) => c.id === "palette.close_project")
    ?.run();
}

function onButtonKeydown(event: KeyboardEvent): void {
  if (event.code === "ArrowDown" || event.code === "ArrowUp") {
    event.preventDefault();
    void show(event.code === "ArrowDown" ? "first" : "last");
  }
}

function onMenuKeydown(event: KeyboardEvent): void {
  if (event.code === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  }
  if (event.code === "Tab") {
    close(false);
    return;
  }
  const next = menuTarget(event, items());
  if (!next) return;
  event.preventDefault();
  next.focus();
}

$effect(() => {
  if (!open) return;
  const onPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (!menu?.contains(target) && !button?.contains(target)) close(false);
  };
  document.addEventListener("pointerdown", onPointerDown, true);
  return () => document.removeEventListener("pointerdown", onPointerDown, true);
});
</script>

<div class="switcher">
  <button
    type="button"
    id="project-button"
    class="project tip"
    data-testid="project-name"
    data-tip={project.root}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls={open ? "project-menu" : undefined}
    aria-label={`Project ${project.name}, switch project`}
    bind:this={button}
    onclick={() => (open ? close(false) : void show("first"))}
    onkeydown={onButtonKeydown}
  >
    <span class="label">{project.name}</span>
    <Icon name="chevron" small />
  </button>
  {#if open && typing}
    <!-- Escape from the field or a button cancels. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form
      class="menu folder"
      aria-label="Open a folder"
      style={place}
      bind:this={menu}
      onkeydown={onFormKeydown}
      onsubmit={(event) => void openTyped(event)}
    >
      <label for="project-menu-path">Folder to open</label>
      <input
        id="project-menu-path"
        bind:this={field}
        bind:value={path}
        spellcheck="false"
        autocomplete="off"
        placeholder="/home/you/my-app"
        aria-invalid={opener.error !== null}
        aria-describedby={opener.error ? "project-menu-path-error" : "project-menu-path-hint"}
      />
      {#if opener.error}
        <p class="error" id="project-menu-path-error" role="alert">{opener.error}</p>
      {:else}
        <p class="hint" id="project-menu-path-hint">{project.name} stays open until the new folder opens.</p>
      {/if}
      <div class="actions">
        <button type="button" onclick={() => close(true)}>Cancel</button>
        <button type="submit" class="primary" disabled={opener.busy || path.trim() === ""}>Open</button>
      </div>
    </form>
  {:else if open}
    <div id="project-menu" class="menu" role="menu" aria-labelledby="project-button" tabindex="-1" style={place} bind:this={menu} onkeydown={onMenuKeydown}>
      {#if recent.length > 0}
        <div role="group" aria-labelledby="project-menu-recent">
          <p class="heading" id="project-menu-recent" aria-hidden="true">Recent projects</p>
          {#each recent as other (other.id)}
            <button
              type="button"
              role="menuitem"
              tabindex="-1"
              aria-label={other.name}
              aria-describedby={`project-menu-root-${other.id}`}
              disabled={opener.busy}
              onclick={() => void switchTo(other)}
            >
              <span class="name">{other.name}</span>
              <span class="path" id={`project-menu-root-${other.id}`}>{other.root}</span>
            </button>
          {/each}
        </div>
        <div role="separator"></div>
      {/if}
      <button type="button" role="menuitem" tabindex="-1" aria-keyshortcuts={folderKeys} onclick={() => void openFolder()}>
        <span class="name">
          Open a folder…
          {#if folderKeys}<kbd aria-hidden="true">{formatShortcut(folderKeys, isApple())}</kbd>{/if}
        </span>
      </button>
      <button type="button" role="menuitem" tabindex="-1" onclick={closeProject}>
        <span class="name">Close project</span>
      </button>
      {#if opener.error}<p class="error" role="alert">{opener.error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .switcher {
    min-width: 0;
  }
  button {
    font: inherit;
    color: var(--text);
    cursor: pointer;
  }
  .project {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    height: var(--ctl);
    padding: 0 8px;
    font-weight: 600;
    background: transparent;
    border: 0;
    border-radius: var(--r-control);
  }
  .project .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .project :global(svg) {
    color: var(--text-3);
  }
  .project:hover,
  .project[aria-expanded="true"] {
    background: var(--s3);
  }
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 300px;
    max-width: min(480px, calc(100vw - 16px));
    padding: 6px;
    background: var(--s4);
    border-radius: var(--radius);
    box-shadow: var(--card-shadow);
  }
  .menu [role="menuitem"] {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    padding: 7px 10px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
  }
  .menu [role="menuitem"]:hover,
  .menu [role="menuitem"]:focus {
    background: var(--s3);
  }
  .menu [role="menuitem"]:disabled {
    cursor: progress;
  }
  .name {
    display: flex;
    align-items: center;
    width: 100%;
  }
  kbd {
    margin-left: auto;
    padding-left: 16px;
    font: 500 12px var(--font);
    color: var(--text-2);
  }
  .heading {
    margin: 6px 10px 4px;
    font-size: var(--fs-ui);
    color: var(--text-2);
  }
  .path {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 13px var(--mono);
    color: var(--text-2);
  }
  [role="separator"] {
    height: 6px;
  }
  .error {
    margin: 4px 10px;
    color: var(--danger);
  }
  .folder {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
  }
  .folder label,
  .hint {
    font-size: var(--fs-ui);
    color: var(--text-2);
  }
  .folder .error,
  .hint {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .folder input {
    height: var(--ctl-lg);
    padding: 0 10px;
    font: 13px var(--mono);
    color: inherit;
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--r-control);
  }
  .folder input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
  .actions button {
    height: var(--ctl);
    padding: 0 12px;
    font-weight: 500;
    color: var(--text-2);
    background: transparent;
    border: 0;
    border-radius: var(--r-control);
  }
  .actions button:hover {
    color: var(--text);
    background: var(--s3);
  }
  .actions .primary,
  .actions .primary:hover {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .actions button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  @media (forced-colors: active) {
    .menu {
      border: 1px solid CanvasText;
    }
  }
</style>
