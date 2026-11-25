<script lang="ts">
// Dev servers: a header button (and the "Servers" command) opens a dialog
// with the servers defined in `.ovra/servers.json` of the open project's main
// checkout or of a worker's worktree, the ones that run there, and their
// output. Escape and the toggle shortcut close it wherever focus is.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import { inBrowserTab, isApple, liveKeys, matchesShortcut, parseShortcut } from "../keys/shortcut";
import type { DevServerId as ServerId, DevServerInfo as ServerInfo, SessionId, StartedBy } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { addServer, CONFIG, checkForm, createConfig, type ServerForm } from "./config";
import LogView from "./LogView.svelte";
import { message, ProjectServers, running, statusText } from "./servers.svelte";

/** Whether to show the header button; the shell opens this from its "More" menu instead. */
let { trigger = true }: { trigger?: boolean } = $props();
const { transport, workspace } = useOvra();
const servers = new ProjectServers(transport);
const sessions = agentSessions(transport);

let opened = $state(false);
let workDir = $state<SessionId | null>(null);
let logsOf = $state(new Set<string>());
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

const TOGGLE_KEYS = "Mod+Shift+S";
type Action = "Stop" | "Restart";
/** The row whose buttons keep focus after an action on it, the buttons it prefers, and the stand-in used while those are off. */
let keep: { row: ServerId; prefer: Action[] } | null = null;
let standIn: HTMLElement | null = null;

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

async function add(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const at = project;
  if (!at || adding) return;
  const checked = checkForm(form, list?.definitions ?? []);
  if ("error" in checked) {
    formError = checked;
    await tick();
    dialog?.querySelector<HTMLElement>(`[data-field=${checked.field}]`)?.focus();
    return;
  }
  adding = true;
  try {
    await addServer(transport, at, checked.def);
    formError = null;
    form = { name: "", command: "", folder: "", port: "5173" };
    await servers.reload();
    await tick();
    // The form is gone with the empty state: go on to the new server's Start.
    dialog?.querySelector<HTMLElement>(`[aria-label="Start ${checked.def.name}"]`)?.focus();
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
  close();
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

$effect(() => () => servers.dispose());
$effect(() => registerCommand({ id: "servers.toggle", title: "Servers", keys: TOGGLE_KEYS, run: toggle }));

// While open, Escape and the toggle chord close the dialog even when focus fell out of it, and
// no other key reaches the panels behind it.
$effect(() => {
  if (!opened) return;
  const chord = parseShortcut(liveKeys(TOGGLE_KEYS, inBrowserTab()), isApple());
  const onKeydown = (event: KeyboardEvent) => {
    if (event.isComposing) return;
    const inside = event.target instanceof Node && !!dialog?.contains(event.target);
    if ((chord && matchesShortcut(event, chord)) || (event.code === "Escape" && !inside)) {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (inside) return;
    event.stopPropagation();
    if (event.code === "Tab") {
      event.preventDefault();
      heading()?.focus();
    }
  };
  window.addEventListener("keydown", onKeydown, true);
  return () => window.removeEventListener("keydown", onKeydown, true);
});

// A status change can disable the focused button: focus moves on to the row's next one.
$effect(() => {
  void list;
  if (opened) void tick().then(placeFocus);
});

function heading(): HTMLElement | null {
  return dialog?.querySelector<HTMLElement>("h2") ?? null;
}

/** Moves focus off a disabled or vanished button to the kept row's preferred enabled one, else its Output. */
function placeFocus(): void {
  if (!keep || !dialog) return;
  const active = document.activeElement;
  const lost =
    !(active instanceof HTMLElement) || !dialog.contains(active) || active.matches(":disabled") || active === standIn;
  if (!lost) return;
  const buttons = [...dialog.querySelectorAll<HTMLButtonElement>(`[data-server="${keep.row}"] .actions button`)];
  const enabled = (name: string) => buttons.find((b) => b.textContent?.trim() === name && !b.disabled);
  const best = keep.prefer.map(enabled).find((b) => b !== undefined);
  const target = best ?? enabled("Output") ?? heading();
  target?.focus();
  standIn = best ? null : target;
}

async function act(row: ServerId | null, prefer: Action[], action: () => Promise<ServerInfo | null>) {
  if (row) keep = { row, prefer };
  const started = await action();
  if (started) keep = { row: started.id, prefer };
  await tick();
  placeFocus();
}

// Read the servers when the dialog opens, and again for another project or work dir.
$effect(() => {
  const shown = { project, workDir };
  if (!opened) return;
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
    logsOf = new Set();
  });
});

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  await tick();
  heading()?.focus();
}

function close(): void {
  opened = false;
  keep = null;
  standIn = null;
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

function toggleLogs(id: string): void {
  const next = new Set(logsOf);
  if (!next.delete(id)) next.add(id);
  logsOf = next;
}

function who(by: StartedBy): string {
  return by.kind === "person" ? "you" : sessions.name(by.session);
}

function where(server: ServerInfo): string {
  return server.work_dir === null ? "project folder" : sessions.name(server.work_dir);
}

function onDialogKeydown(event: KeyboardEvent): void {
  // Shortcuts of the panels behind the modal must not act.
  event.stopPropagation();
  if (event.code === "Escape") {
    event.preventDefault();
    close();
    return;
  }
  if (event.code !== "Tab") return;
  // Keep Tab inside the dialog (aria-modal).
  const focusable = [
    ...(dialog?.querySelectorAll<HTMLElement>("button, input, select, summary, [tabindex]") ?? []),
  ].filter((el) => el.tabIndex >= 0 && !el.matches(":disabled"));
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  const at = focusable.indexOf(document.activeElement as HTMLElement);
  if (at < 0 || (event.shiftKey && at === 0) || (!event.shiftKey && at === focusable.length - 1)) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  }
}
</script>

{#if trigger}
  <button type="button" class="open-servers" data-testid="open-servers" aria-haspopup="dialog" onclick={() => void show()}>
    Servers
  </button>
{/if}

{#if opened}
  <div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && event.preventDefault()}>
    <div
      class="servers"
      role="dialog"
      aria-modal="true"
      aria-labelledby="servers-title"
      tabindex="-1"
      data-testid="servers"
      bind:this={dialog}
      onkeydown={onDialogKeydown}
    >
      <header>
        <h2 id="servers-title" tabindex="-1">Servers</h2>
        <span class="muted">{workspace.project?.name ?? "No project"}</span>
        {#if project && (workers.length > 0 || workDir !== null)}
          <!-- Only a choice once a worker has its own copy of the project. -->
          <label>
            Runs in
            <select bind:value={workDir} data-testid="servers-work-dir">
              <option value={null}>Project folder</option>
              {#each workers as worker (worker.id)}
                <option value={worker.id}>{worker.name}</option>
              {/each}
            </select>
          </label>
        {/if}
        <button type="button" class="close" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      <div class="body">
        {#if list?.definitions.length !== 0}
          <!-- The empty state says it in its own words. -->
          <p class="muted intro">
            Your project's dev servers, run in the sandbox so agents working in the same folder can reach them.
          </p>
        {/if}
        {#if !project}
          <p class="muted">Open a project to run its dev servers.</p>
        {:else if servers.unavailable}
          <p class="warn" role="note" data-testid="servers-unavailable">
            Servers cannot run here: they need the Linux sandbox, which keeps the servers of each folder agents
            work in apart. ({servers.unavailable})
          </p>
        {:else}
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
              <p><strong>No dev servers yet.</strong> Add one so agents can open your app while they work.</p>
              {#if workDir === null}
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
                  <button type="submit" aria-busy={adding}>{adding ? "Adding…" : "Add server"}</button>
                </form>
                <p class="muted" id="servers-add-note">
                  The command runs as typed, without a shell, in the sandbox. Ovra sets <code>PORT</code> to the port, so
                  your server should listen on it. This writes <code>{CONFIG}</code>, which only you can change.
                </p>
                {#if formError}
                  <p class="error" role="alert" id="servers-add-error" data-testid="servers-add-error">{formError.error}</p>
                {/if}
              {:else}
                <p class="muted">This worker's servers come from <code>{CONFIG}</code> in its own copy of the project.</p>
              {/if}
              <details>
                <summary>Write the file by hand</summary>
                <pre>{example}</pre>
                <p class="actions-row">
                  <button type="button" onclick={() => void copyExample()}>{copied ? "Copied" : "Copy example"}</button>
                  {#if workDir === null}
                    <button type="button" onclick={() => void openConfig()}>Open in the editor</button>
                  {/if}
                </p>
              </details>
            </div>
          {:else}
            <ul aria-label="Defined servers">
              {#each list.definitions as def (def.name)}
                {@const latest = servers.latestOf(def.name)}
                <li data-testid="server-def">
                  <strong>{def.name}</strong>
                  <code class="muted">{def.command.join(" ")}</code>
                  <span class="muted">port {def.port}</span>
                  <button
                    type="button"
                    disabled={!!latest && running(latest.status)}
                    aria-label={`Start ${def.name}`}
                    onclick={() => void act(null, ["Stop", "Restart"], () => servers.start(def.name))}>Start</button
                  >
                </li>
              {/each}
            </ul>
            {#if workDir === null}
              <button type="button" class="link" onclick={() => void openConfig()}>Edit {CONFIG}</button>
            {/if}
          {/if}
          {#if list && list.servers.length > 0}
            <h3>Started</h3>
            <ul aria-label="Started servers">
              {#each [...list.servers].reverse() as server (server.id)}
                <li class="server" data-testid="server" data-server={server.id}>
                  <div class="row">
                    <strong>{server.name}</strong>
                    <span class={`badge ${server.status.state}`} data-testid="server-status">
                      {statusText(server.status)}
                    </span>
                    <span class="muted">port {server.port}</span>
                    <span class="muted">in {where(server)}</span>
                    <span class="muted">started by {who(server.started_by)}</span>
                    <span class="muted">run {server.generation}</span>
                    <span class="actions">
                      <button
                        type="button"
                        aria-expanded={logsOf.has(server.id)}
                        onclick={() => toggleLogs(server.id)}>Output</button
                      >
                      <button
                        type="button"
                        aria-label={`Restart ${server.name}`}
                        disabled={server.status.state === "stopping"}
                        onclick={() => void act(server.id, ["Restart", "Stop"], () => servers.restart(server.id))}
                        >Restart</button
                      >
                      <button
                        type="button"
                        aria-label={`Stop ${server.name}`}
                        disabled={!running(server.status) || server.status.state === "stopping"}
                        onclick={() => void act(server.id, ["Restart"], () => servers.stop(server.id))}>Stop</button
                      >
                    </span>
                  </div>
                  {#if logsOf.has(server.id)}
                    <LogView id={server.id} name={server.name} />
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  button,
  input,
  select {
    font: inherit;
    padding: 2px 10px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  input::placeholder {
    color: #9a9aa8;
    font-style: italic;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible,
  summary:focus-visible,
  h2:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    justify-content: center;
    padding: 24px;
    background: rgb(0 0 0 / 55%);
  }
  .servers {
    flex: 1;
    max-width: 760px;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: #16161a;
    border: 1px solid #3a3a44;
    border-radius: 6px;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid #2a2a31;
  }
  .close {
    margin-left: auto;
  }
  h2 {
    margin: 0;
    font-size: 14px;
  }
  h3 {
    margin: 12px 0 4px;
    font-size: 13px;
  }
  label {
    font-size: 12px;
    color: #c8c8d2;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 8px 12px 12px;
  }
  .intro {
    margin: 0 0 8px;
  }
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .error {
    color: #ff8f8f;
  }
  .warn {
    color: #ffd36b;
  }
  pre {
    padding: 8px;
    font-size: 12px;
    background: #101013;
    border: 1px solid #2a2a31;
    border-radius: 3px;
    overflow: auto;
  }
  ul {
    list-style: none;
    margin: 6px 0;
    padding: 0;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    padding: 4px 0;
  }
  li > button:last-child,
  .actions {
    margin-left: auto;
  }
  .server {
    display: block;
    border-bottom: 1px solid #24242b;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    padding-bottom: 4px;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .badge {
    padding: 0 6px;
    font-size: 12px;
    border-radius: 8px;
    background: #2a2a33;
  }
  .ready {
    color: #7ee787;
  }
  .starting,
  .stopping {
    color: #f2cc60;
  }
  .failed {
    color: #ff8f8f;
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
    gap: 2px;
  }
  .add input {
    width: 9em;
  }
  .add input[data-field="command"] {
    width: 14em;
  }
  details {
    margin-top: 10px;
    font-size: 12px;
    color: #b4b4c0;
  }
  summary {
    cursor: pointer;
    width: fit-content;
  }
  .actions-row {
    display: flex;
    gap: 8px;
    margin: 0;
  }
  .link {
    padding: 2px 0;
    color: #9db7ff;
    background: none;
    border: none;
    text-decoration: underline;
  }
</style>
