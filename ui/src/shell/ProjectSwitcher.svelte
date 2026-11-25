<script lang="ts">
// The header's project name, a menu button: the other recent projects,
// "Open a folder…" and "Close project". Keys as in the More menu: arrows,
// Home, End and a first letter move; Escape closes and gives focus back.
import { tick } from "svelte";
import { listCommands } from "../keys/commands";
import { formatShortcut, inBrowserTab, isApple, liveKeys } from "../keys/shortcut";
import type { Project } from "../lib/protocol";
import { FolderOpener } from "../tree/folder-opener.svelte";
import { useOvra } from "./context";
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

function items(): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
}

async function show(focus: "first" | "last"): Promise<void> {
  const at = button?.getBoundingClientRect();
  place = at ? `top: ${at.bottom + 4}px; left: ${at.left}px;` : "";
  opener.error = null;
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
  if (refocus) button?.focus();
}

async function switchTo(other: Project): Promise<void> {
  await opener.openKnown(other);
  // A failure stays in the menu, next to the project that did not open.
  if (!opener.error) close(true);
}

function openFolder(): void {
  close(true);
  void openAnotherFolder(workspace);
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
    class="project"
    data-testid="project-name"
    title={project.root}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls={open ? "project-menu" : undefined}
    aria-label={`Project ${project.name}, switch project`}
    bind:this={button}
    onclick={() => (open ? close(false) : void show("first"))}
    onkeydown={onButtonKeydown}
  >
    <span class="muted">Project</span>
    {project.name}
    <span class="caret" aria-hidden="true">▾</span>
  </button>
  {#if open}
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
      <button type="button" role="menuitem" tabindex="-1" aria-keyshortcuts={folderKeys} onclick={openFolder}>
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
    color: #e6e6e6;
    cursor: pointer;
  }
  .project {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 2px 6px;
    font-weight: 600;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
  }
  .project:hover,
  .project[aria-expanded="true"] {
    background: #2a2a33;
    border-color: #4a4a57;
  }
  button:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .muted,
  .caret {
    font-weight: normal;
    color: #9a9aa6;
  }
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 280px;
    max-width: min(480px, calc(100vw - 16px));
    padding: 4px;
    background: #1e1e24;
    border: 1px solid #4a4a57;
    border-radius: 4px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
  }
  .menu button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    padding: 6px 8px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
  }
  .menu button:hover,
  .menu button:focus {
    background: #2a2a33;
  }
  .menu button:disabled {
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
    font: inherit;
    font-size: 12px;
    color: #9a9aa6;
  }
  .heading {
    margin: 4px 8px 2px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #9a9aa6;
  }
  .path {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: #9a9aa6;
  }
  [role="separator"] {
    margin: 4px 0;
    border-top: 1px solid #2a2a31;
  }
  .error {
    margin: 4px 8px;
    color: #ff6b6b;
  }
</style>
