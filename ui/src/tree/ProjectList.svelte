<script lang="ts">
// Start screen: the known projects, most recently opened first, and a field
// to add one by its folder path.
import type { Project } from "../lib/protocol";
import type { Transport } from "../lib/transport";

let { transport, onopen }: { transport: Transport; onopen: (project: Project) => void } = $props();

let projects = $state<Project[] | null>(null);
let error = $state<string | null>(null);
let root = $state("");

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

$effect(() => {
  transport
    .call({ method: "projects.list" })
    .then((r) => {
      projects = r.data;
    })
    .catch((err: unknown) => {
      error = message(err);
    });
});

async function open(project: Project): Promise<void> {
  try {
    onopen((await transport.call({ method: "projects.open", params: { id: project.id } })).data);
  } catch (err) {
    error = message(err);
  }
}

async function add(e: SubmitEvent): Promise<void> {
  e.preventDefault();
  const path = root.trim();
  if (!path) return;
  try {
    const project = (await transport.call({ method: "projects.add", params: { root: path } })).data;
    root = "";
    await open(project);
  } catch (err) {
    error = message(err);
  }
}
</script>

<section class="start" data-testid="project-list">
  <h2>Projects</h2>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if projects?.length === 0}
    <p class="note">No projects yet.</p>
  {:else if projects}
    <ul>
      {#each projects as project (project.id)}
        <li>
          <button type="button" title={project.root} onclick={() => open(project)}>{project.name}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <form onsubmit={add}>
    <input aria-label="Project folder" placeholder="/path/to/project" bind:value={root} />
    <button type="submit">Add</button>
  </form>
</section>

<style>
  .start {
    padding: 12px;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #9a9aa6;
  }
  ul {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
  }
  li button {
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
  form {
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
  form button {
    padding: 4px 10px;
    border: 1px solid #2a2a31;
    background: #22222a;
    color: inherit;
    font: inherit;
  }
  .note {
    color: #6f6f7a;
  }
  .error {
    color: #ff6b6b;
  }
</style>
