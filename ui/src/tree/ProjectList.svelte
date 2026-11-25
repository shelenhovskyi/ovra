<script lang="ts">
// Start screen: the known projects, most recently opened first, a button for
// the system's folder dialog where there is one, and a field to open a folder
// by its path.
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { type FolderOpener, folderError } from "./folder-opener.svelte";

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
  <h2>Projects</h2>
  {#if projects?.length === 0}
    <p class="muted">No projects yet.</p>
  {:else if projects}
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
          >
            <span class="name">{project.name}</span>
            <span class="path">{project.root}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if opener.canPick}
    <button type="button" class="pick" bind:this={pickButton} disabled={opener.busy} onclick={() => opener.pick()}>Open folder…</button>
    <p class="muted hint drop-hint">Or drop a folder here.</p>
  {/if}
  <form onsubmit={add}>
    <label for="project-root">{opener.canPick ? "Or type its path" : "Open a folder"}</label>
    <div class="row">
      <input
        id="project-root"
        bind:this={input}
        bind:value={root}
        oninput={edited}
        aria-invalid={error ? "true" : undefined}
        aria-describedby={error ? "project-root-error project-root-hint" : "project-root-hint"}
      />
      <button type="submit" disabled={opener.busy}>{opener.busy ? "Opening…" : "Open"}</button>
    </div>
    <p id="project-root-hint" class="muted hint">Full path, like /home/you/my-app</p>
  </form>
  {#if error}<p id="project-root-error" class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .start {
    padding: 14px;
    background: var(--s1);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
  }
  ul {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
  }
  li button {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  li button:hover {
    background: var(--s3);
  }
  .name {
    font-weight: 500;
  }
  .path {
    color: var(--text-2);
    font: 13px var(--mono);
    overflow-wrap: anywhere;
  }
  label {
    display: block;
    margin-bottom: 6px;
    color: var(--text-2);
    font-size: 13px;
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
    border-radius: var(--radius-sm);
    background: var(--s3);
    color: inherit;
    font: 13px var(--mono);
  }
  input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .pick {
    width: 100%;
  }
  .drop-hint {
    margin-bottom: 16px;
  }
  form button,
  .pick {
    padding: 6px 12px;
    border: 0;
    border-radius: var(--radius-sm);
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
    font-size: 13px;
  }
  .error {
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
