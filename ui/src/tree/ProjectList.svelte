<script lang="ts">
// Start screen: the known projects, most recently opened first, a button for
// the system's folder dialog where there is one (and a place to drop a
// folder), and a field to open a folder by its path.
import { formatShortcut, isApple } from "../keys/shortcut";
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import Icon from "../shell/Icon.svelte";
import { type FolderOpener, folderError } from "./folder-opener.svelte";
import { openedAgo, shortPath } from "./start";

let { transport, opener }: { transport: Transport; opener: FolderOpener } = $props();

let projects = $state<Project[] | null>(null);
let loadError = $state<string | null>(null);
let root = $state("");
let focused = $state(0);
let input = $state<HTMLInputElement>();
let pickButton = $state<HTMLButtonElement>();
let list = $state<HTMLUListElement>();

const KEYS = ["ArrowDown", "ArrowUp", "Home", "End"];
const error = $derived(opener.error ?? loadError);
const pickKeys = formatShortcut("Mod+O", isApple());
const now = Date.now();

function load(): void {
  transport
    .call({ method: "projects.list" })
    .then((r) => {
      projects = r.data;
      loadError = null;
    })
    .catch((err: unknown) => {
      loadError = folderError(err);
    });
}

$effect(() => {
  load();
  // Load again when the core comes back after a lost connection.
  let wasClosed = false;
  return transport.onStatus?.((status) => {
    if (status === "closed") wasClosed = true;
    else if (status === "open" && wasClosed) {
      wasClosed = false;
      load();
    }
  });
});

$effect(() => {
  if (projects?.length === 0) (pickButton ?? input)?.focus();
  // The list loaded with focus nowhere (a fresh window): the first project takes it, so Enter opens it.
  else if (projects?.length && (!document.activeElement || document.activeElement === document.body))
    list?.querySelector<HTMLButtonElement>("button")?.focus();
});

function open(project: Project): void {
  loadError = null;
  void opener.openKnown(project);
}

async function add(e: SubmitEvent): Promise<void> {
  e.preventDefault();
  const path = root.trim();
  if (!path) return;
  if (await opener.open(path)) root = "";
}

function edited(): void {
  opener.error = null;
  loadError = null;
}

function move(e: KeyboardEvent): void {
  if (!projects?.length || !KEYS.includes(e.key)) return;
  e.preventDefault();
  const last = projects.length - 1;
  if (e.key === "ArrowDown") focused = Math.min(focused + 1, last);
  else if (e.key === "ArrowUp") focused = Math.max(focused - 1, 0);
  else focused = e.key === "Home" ? 0 : last;
  list?.querySelectorAll<HTMLButtonElement>("button")[focused]?.focus();
}
</script>

<section class="start" data-testid="project-list">
  <h2>Open a project</h2>
  {#if projects?.length === 0}
    <p class="lead">No projects yet. Open a folder, describe a task for an agent, then approve its steps and review its changes.</p>
  {:else if projects}
    <p class="lead">Pick up where you left off.</p>
    <h3 id="recent">Recent</h3>
    <!-- Arrow keys bubble up from the project buttons. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul aria-labelledby="recent" bind:this={list} onkeydown={move}>
      {#each projects as project, i (project.id)}
        <li>
          <button
            type="button"
            tabindex={i === focused ? 0 : -1}
            onfocus={() => (focused = i)}
            onclick={() => open(project)}
            title={project.root}
          >
            <span class="initial" aria-hidden="true">{project.name.slice(0, 1).toUpperCase()}</span>
            <span class="text">
              <span class="name">{project.name}</span>
              <span class="path">{shortPath(project.root)}</span>
            </span>
            <span class="when">{openedAgo(project.last_opened_ms, now)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if opener.canPick}
    <div class="drop">
      <span class="tile"><Icon name="folder" /></span>
      <span class="muted">Drop a folder here</span>
      <button type="button" class="pick" bind:this={pickButton} disabled={opener.busy} onclick={() => opener.pick()}
        >Open folder…<kbd aria-hidden="true">{pickKeys}</kbd></button
      >
    </div>
  {/if}
  <form onsubmit={add}>
    <label for="project-root">{opener.canPick ? "Or type its path" : "Open a folder"}</label>
    <div class="row">
      <input
        id="project-root"
        bind:this={input}
        bind:value={root}
        oninput={edited}
        spellcheck="false"
        autocomplete="off"
        aria-invalid={error ? "true" : undefined}
        aria-describedby={error ? "project-root-error" : "project-root-hint"}
      />
      <button type="submit" disabled={opener.busy}>{opener.busy ? "Opening…" : "Open"}</button>
    </div>
    <!-- The error replaces the hint rather than repeating it. -->
    {#if error}
      <p id="project-root-error" class="error" role="alert">{error}</p>
    {:else}
      <p id="project-root-hint" class="muted hint">Full path, like /home/you/my-app</p>
    {/if}
  </form>
</section>

<style>
  /* One column of about 560 px, set in from the window's edge as in the mockup. */
  .start {
    max-width: 560px;
    padding: 64px 32px 32px 64px;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 24px;
    font-weight: 600;
  }
  .lead {
    margin: 0 0 32px;
    color: var(--text-2);
  }
  h3 {
    margin: 0 0 6px;
    font-size: var(--fs-ui);
    font-weight: 500;
    color: var(--text-2);
  }
  ul {
    list-style: none;
    margin: 0 0 24px -8px;
    padding: 0;
  }
  li button {
    display: grid;
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  li button:hover {
    background: var(--s3);
  }
  .initial,
  .tile {
    display: grid;
    place-items: center;
    width: var(--ctl-lg);
    height: var(--ctl-lg);
    font-size: var(--fs-ui);
    font-weight: 600;
    color: var(--text-2);
    background: var(--s3);
    border-radius: var(--r-control);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
    font: 13px var(--mono);
  }
  .when {
    font-size: var(--fs-ui);
    color: var(--text-2);
    white-space: nowrap;
  }
  /* The drop target needs an edge to be found. */
  .drop {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 24px;
    padding: 16px;
    border: 1px dashed var(--s4);
    border-radius: var(--radius);
  }
  .drop .muted {
    flex: 1;
  }
  .pick {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  kbd {
    padding: 2px 6px;
    font: 500 12px var(--font);
    background: color-mix(in srgb, var(--accent-ink) 15%, transparent);
    border-radius: 5px;
  }
  label {
    display: block;
    margin-bottom: 6px;
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
  .row {
    display: flex;
    gap: 6px;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 6px 10px;
    border: 1px solid transparent;
    border-radius: var(--r-control);
    background: var(--s3);
    color: inherit;
    font: 13px var(--mono);
  }
  input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  form button,
  .pick {
    padding: 6px 12px;
    border: 0;
    border-radius: var(--r-control);
    background: var(--accent);
    color: var(--accent-ink);
    font: inherit;
    font-weight: 500;
  }
  form button {
    background: var(--s3);
    color: var(--text);
  }
  form button:hover {
    background: var(--s4);
  }
  /* Without a folder dialog, Open is the one primary action. */
  .start:not(:has(.pick)) form button {
    background: var(--accent);
    color: var(--accent-ink);
  }
  form button:disabled,
  .pick:disabled {
    cursor: progress;
  }
  .muted {
    color: var(--text-2);
  }
  .hint {
    margin: 6px 0 0;
    font-size: var(--fs-ui);
  }
  .error {
    margin: 6px 0 0;
    font-size: var(--fs-ui);
    color: var(--danger);
  }

  @media (forced-colors: active) {
    input,
    form button,
    .pick {
      border: 1px solid ButtonText;
    }
  }
</style>
