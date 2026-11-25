<script lang="ts">
// Dev server definitions, the Servers pane of Settings: the servers defined
// in `.ovra/servers.json` of the open project's main checkout or of a
// worker's worktree, a form that adds one, and Start. What runs, and its
// output, is in the Servers tab of the bottom dock (ServersDock).
import { tick, untrack } from "svelte";
import { NO_SANDBOX } from "../agents/sandbox";
import { agentSessions } from "../agents/sessions.svelte";
import type { SessionId } from "../lib/protocol";
import { projectSandbox } from "../settings/sandbox";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import { mainView } from "../shell/main-view.svelte";
import { addServer, CONFIG, checkForm, createConfig, type ServerForm } from "./config";
import { serversDock } from "./dock.svelte";
import { message, ProjectServers, running } from "./servers.svelte";

const { transport, workspace } = useOvra();
const servers = new ProjectServers(transport);
const sessions = agentSessions(transport);

let workDir = $state<SessionId | null>(null);
let pane = $state<HTMLElement>();

const project = $derived(workspace.project?.id ?? null);
const workers = $derived(
  project
    ? [...sessions.roles]
        .filter(([id, role]) => role.kind === "worker" && sessions.byId.get(id)?.project === project)
        .map(([id]) => ({ id, name: sessions.name(id) }))
    : [],
);
const list = $derived(servers.list);
const example = `{
  "servers": [
    { "name": "web", "command": ["pnpm", "dev"], "port": 5173, "ready": { "log": "ready in" } }
  ]
}`;

/** The add form's fields, each with an example. */
const FIELDS = [
  { field: "name", label: "Name", hint: "web" },
  { field: "command", label: "Command", hint: "pnpm dev" },
  { field: "folder", label: "Folder (optional)", hint: "ui" },
  { field: "port", label: "Port", hint: "5173" },
] as const;

let form = $state<ServerForm>({ name: "", command: "", folder: "", port: "5173" });
let formError = $state<{ field: keyof ServerForm; error: string } | null>(null);
let adding = $state(false);
let copied = $state(false);
/** The add form under the list, once servers are defined. */
let addOpen = $state(false);
/** How agents run here, as the session says it. */
const sandbox = $derived(project ? projectSandbox(sessions.inProject(project)) : null);

async function openAdd(): Promise<void> {
  addOpen = true;
  await tick();
  pane?.querySelector<HTMLElement>("[data-field=name]")?.focus();
}

$effect(() => () => servers.dispose());

// Read the servers, and again for another project or work dir.
$effect(() => {
  const shown = { project, workDir };
  untrack(() => {
    void servers.show(shown.project, shown.workDir);
    if (shown.project) sessions.load(shown.project).catch(() => {});
  });
});

// Another project has other workers.
$effect(() => {
  void project;
  untrack(() => {
    workDir = null;
  });
});

async function add(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const at = project;
  if (!at || adding) return;
  const checked = checkForm(form, list?.definitions ?? []);
  if ("error" in checked) {
    formError = checked;
    await tick();
    pane?.querySelector<HTMLElement>(`[data-field=${checked.field}]`)?.focus();
    return;
  }
  adding = true;
  try {
    await addServer(transport, at, checked.def);
    formError = null;
    form = { name: "", command: "", folder: "", port: "5173" };
    addOpen = false;
    await servers.reload();
    await tick();
    // The form folds away under the list: go on to the new server's Start.
    pane?.querySelector<HTMLElement>(`[aria-label="Start ${checked.def.name}"]`)?.focus();
  } catch (err) {
    formError = { field: "name", error: `Could not add the server: ${message(err)}` };
  } finally {
    adding = false;
  }
}

/** Opens the servers file in the editor, starting it from the example if there is none. */
async function openConfig(): Promise<void> {
  const at = project;
  if (!at) return;
  try {
    await createConfig(transport, at, `${example}\n`);
  } catch (err) {
    formError = { field: "name", error: `Could not create ${CONFIG}: ${message(err)}` };
    return;
  }
  mainView.close(false);
  workspace.openFile(CONFIG, "edit");
}

async function copyExample(): Promise<void> {
  try {
    await navigator.clipboard.writeText(example);
    copied = true;
  } catch {
    copied = false;
  }
}

/** Shows this work dir's servers in the dock, where they run; the overview of all folders shows them too. */
function showRunning(): void {
  if (!serversDock.everywhere) serversDock.workDir = workDir;
  serversDock.shown = true;
}

async function start(name: string): Promise<void> {
  if (!(await servers.start(name))) return;
  serversDock.started++;
  showRunning();
}
</script>

{#snippet addForm()}
  <form class="add" onsubmit={add} aria-label="Add a server" data-testid="servers-add">
    {#each FIELDS as input (input.field)}
      <label>
        {input.label}
        <input
          bind:value={form[input.field]}
          data-field={input.field}
          placeholder={`For example ${input.hint}`}
          inputmode={input.field === "port" ? "numeric" : undefined}
          autocomplete="off"
          spellcheck="false"
          aria-invalid={formError?.field === input.field ? "true" : undefined}
          aria-describedby={formError?.field === input.field ? "servers-add-error" : "servers-add-note"}
        />
      </label>
    {/each}
    <button type="submit" class="primary" aria-busy={adding}>{adding ? "Adding…" : "Add server"}</button>
  </form>
  <p class="muted" id="servers-add-note">Saved to <code>{CONFIG}</code>, which sandboxed agents can't change.</p>
  {#if formError}
    <p class="error" role="alert" id="servers-add-error" data-testid="servers-add-error">{formError.error}</p>
  {/if}
{/snippet}

{#snippet details()}
  <details>
    <summary>Details</summary>
    <p>Runs as typed, without a shell; it should listen on <code>PORT</code>. Or write the file by hand:</p>
    <pre>{example}</pre>
    <p class="actions-row">
      <button type="button" onclick={() => void copyExample()}>{copied ? "Copied" : "Copy example"}</button>
      {#if workDir === null}
        <button type="button" onclick={() => void openConfig()}>Open in the editor</button>
      {/if}
    </p>
  </details>
{/snippet}

<div class="servers" data-testid="servers-settings" bind:this={pane}>
  {#if sandbox === "none"}
    <!-- The session's own words: the pane promises no sandbox this computer lacks. -->
    <p class="warn note" role="note" data-testid="servers-unsandboxed">{NO_SANDBOX}.</p>
  {/if}
  {#if project && (workers.length > 0 || workDir !== null)}
    <!-- Only a choice once a worker has its own copy of the project. -->
    <label class="where">
      Defined in
      <select bind:value={workDir} data-testid="servers-work-dir">
        <option value={null}>Project folder</option>
        {#each workers as worker (worker.id)}
          <option value={worker.id}>{worker.name}</option>
        {/each}
      </select>
    </label>
  {/if}
  {#if servers.unavailable}
    <p class="warn" role="note" data-testid="servers-unavailable">
      Servers cannot run here: they need the Linux sandbox. ({servers.unavailable})
    </p>
  {:else if project}
    {#if servers.error}
      <p class="error" role="alert">{servers.error}</p>
    {/if}
    {#if list?.error}
      <p class="error" role="alert" data-testid="servers-file-error">
        Could not read <code>.ovra/servers.json</code>: {list.error}
      </p>
      {#if workDir === null}
        <button type="button" onclick={() => void openConfig()}>Open it in the editor</button>
      {/if}
    {/if}
    {#if !list}
      <p class="muted">Loading…</p>
    {:else if list.definitions.length === 0 && !list.error}
      <div class="empty" data-testid="servers-empty">
        <EmptyState headline="No dev servers" line="Add the command that starts your app, so agents can open it." />
        {#if workDir === null}
          {@render addForm()}
        {:else}
          <p class="muted">This worker's servers come from <code>{CONFIG}</code> in its own copy of the project.</p>
        {/if}
        {@render details()}
      </div>
    {:else}
      <ul aria-label="Defined servers">
        {#each list.definitions as def (def.name)}
          {@const latest = servers.latestOf(def.name)}
          {@const runs = !!latest && running(latest.status)}
          <li data-testid="server-def">
            <strong>{def.name}</strong>
            <code class="muted">{def.command.join(" ")}</code>
            <span class="muted">port {def.port}</span>
            <!-- A start that failed says so here; the dock shows its output, and why. -->
            {#if latest && servers.failed(latest)}
              <span class="warn" data-testid="server-failed">{servers.statusOf(latest)}</span>
              <button type="button" aria-label={`Output of ${def.name}`} onclick={showRunning}>Output</button>
            {/if}
            <!-- One button: while the server runs, it shows it in the dock instead of starting another. -->
            <button
              type="button"
              class="end"
              aria-label={`${runs ? "Show" : "Start"} ${def.name}`}
              onclick={() => (runs ? showRunning() : void start(def.name))}>{runs ? "Show" : "Start"}</button
            >
          </li>
        {/each}
      </ul>
      {#if workDir === null}
        {#if addOpen}
          {@render addForm()}
        {:else}
          <p class="actions-row">
            <button type="button" data-testid="servers-add-another" onclick={openAdd}>Add server…</button>
            <button type="button" class="link" onclick={() => void openConfig()}>Edit {CONFIG}</button>
          </p>
        {/if}
      {/if}
    {/if}
  {/if}
</div>

<style>
  .muted,
  label {
    color: var(--text-2);
    font: var(--t-small);
  }
  .where {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  button,
  input,
  select {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  button {
    font-weight: 500;
    cursor: default;
  }
  button:hover {
    background: var(--s4);
  }
  button.primary {
    color: var(--accent-ink);
    background: var(--accent);
  }
  .error {
    color: var(--danger);
    margin: 6px 0;
  }
  .warn {
    color: var(--warn);
  }
  .empty > :global(.empty) {
    padding: 0 0 8px;
  }
  pre {
    padding: 10px 12px;
    font: 13px / 18px var(--mono);
    background: var(--s1);
    border-radius: var(--r-md);
    overflow: auto;
  }
  ul {
    list-style: none;
    display: grid;
    gap: 4px;
    margin: 0 0 8px;
    padding: 0;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--r-md);
    background: var(--s1);
  }
  li > .end,
  li > .warn {
    margin-left: auto;
  }
  li > .warn ~ .end {
    margin-left: 0;
  }
  p.warn {
    margin: 0 0 8px;
    padding: 8px 12px;
    font: var(--t-small);
    border-radius: var(--r-md);
    background: var(--warn-soft);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
    margin: 8px 0 4px;
  }
  .add label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .add input {
    width: 9em;
  }
  .add input[data-field="command"] {
    width: 14em;
  }
  details {
    margin-top: 10px;
    font: var(--t-small);
    color: var(--text-2);
  }
  summary {
    width: fit-content;
    cursor: default;
  }
  .actions-row {
    display: flex;
    gap: 8px;
    margin: 0;
  }
  .link {
    padding: 2px 0;
    color: var(--accent);
    background: none;
    text-decoration: underline;
  }
  @media (forced-colors: active) {
    button,
    input,
    select {
      border: 1px solid ButtonText;
    }
  }
</style>
