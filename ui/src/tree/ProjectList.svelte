<script lang="ts">
// Start screen: the known projects, most recently opened first, and a field
// to open a folder by its path.
import type { Project } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

let { transport, onopen }: { transport: Transport; onopen: (project: Project) => void } = $props();

let projects = $state<Project[] | null>(null);
let error = $state<string | null>(null);
let root = $state("");
let busy = $state(false);
let focused = $state(0);
let input = $state<HTMLInputElement>();
let list = $state<HTMLUListElement>();

const ABSOLUTE = /^(\/|[A-Za-z]:[\\/])/;
const KEYS = ["ArrowDown", "ArrowUp", "Home", "End"];

/** A message for people: no protocol code, and a hint where there is one. */
function message(err: unknown, path?: string): string {
  if (!(err instanceof CoreError)) return err instanceof Error ? err.message : String(err);
  switch (err.code) {
    case "not_found":
      return path ? `Folder not found: ${path}` : err.error.message;
    case "bad_request":
      return path && /not a (folder|directory)/i.test(err.error.message)
        ? "That is not a folder. Choose the folder itself, not a file in it."
        : err.error.message;
    case "unavailable":
      return "The project store is not available right now.";
    default:
      return err.error.message;
  }
}

function load(): void {
  transport
    .call({ method: "projects.list" })
    .then((r) => {
      projects = r.data;
      error = null;
    })
    .catch((err: unknown) => {
      error = message(err);
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
  if (projects?.length === 0) input?.focus();
});

async function open(project: Project): Promise<void> {
  try {
    onopen((await transport.call({ method: "projects.open", params: { id: project.id } })).data);
  } catch (err) {
    error = message(err, project.root);
  }
}

async function add(e: SubmitEvent): Promise<void> {
  e.preventDefault();
  error = null;
  const path = root.trim();
  if (!path || busy) return;
  if (!ABSOLUTE.test(path)) {
    error = "Use the full path to a folder, for example /home/you/my-app.";
    return;
  }
  busy = true;
  try {
    const project = (await transport.call({ method: "projects.add", params: { root: path } })).data;
    root = "";
    await open(project);
  } catch (err) {
    error = message(err, path);
  } finally {
    busy = false;
  }
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
  <form onsubmit={add}>
    <label for="project-root">Open a folder</label>
    <div class="row">
      <input
        id="project-root"
        bind:this={input}
        bind:value={root}
        oninput={() => (error = null)}
        aria-invalid={error ? "true" : undefined}
        aria-describedby={error ? "project-root-error project-root-hint" : "project-root-hint"}
      />
      <button type="submit" disabled={busy}>{busy ? "Opening…" : "Open"}</button>
    </div>
    <p id="project-root-hint" class="muted hint">Full path to a folder, for example /home/you/my-app</p>
  </form>
  {#if error}<p id="project-root-error" class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .start {
    padding: 12px;
  }
  h2,
  h3 {
    margin: 0 0 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #9a9aa6;
  }
  h3 {
    font-size: 11px;
  }
  ul {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
  }
  li button {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 4px 6px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  li button:hover {
    background: #1f1f25;
  }
  .path {
    color: #9a9aa6;
    font-size: 0.9em;
    overflow-wrap: anywhere;
  }
  label {
    display: block;
    margin-bottom: 4px;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 4px 6px;
    border: 1px solid #2a2a31;
    background: #1b1b20;
    color: inherit;
    font: inherit;
  }
  input[aria-invalid="true"] {
    border-color: #ff6b6b;
  }
  form button {
    padding: 4px 10px;
    border: 1px solid #2a2a31;
    background: #22222a;
    color: inherit;
    font: inherit;
  }
  form button:disabled {
    cursor: progress;
  }
  .muted {
    color: #9a9aa6;
  }
  .hint {
    margin: 4px 0 0;
    font-size: 0.9em;
  }
  .error {
    color: #ff6b6b;
  }
</style>
