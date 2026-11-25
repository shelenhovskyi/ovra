<script lang="ts">
// Dev servers: a header button (and the "Servers" command) opens a dialog
// with the servers defined in `.ovra/servers.json` of the open project's main
// checkout or of a worker's worktree, the ones that run there, and their
// output. Escape closes.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import type { DevServerInfo as ServerInfo, SessionId, StartedBy } from "../lib/protocol";
import { useOvra } from "../shell/context";
import LogView from "./LogView.svelte";
import { ProjectServers, running, statusText } from "./servers.svelte";

const { transport, workspace } = useOvra();
const servers = new ProjectServers(transport);
const sessions = agentSessions(transport);

let opened = $state(false);
let workDir = $state<SessionId | null>(null);
let logsOf = $state(new Set<string>());
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

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

$effect(() => () => servers.dispose());
$effect(() => registerCommand({ id: "servers.toggle", title: "Servers", keys: "Mod+Shift+S", run: toggle }));

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
  dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
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
  return server.work_dir === null ? "main checkout" : sessions.name(server.work_dir);
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
  const focusable = [...(dialog?.querySelectorAll<HTMLElement>("button, select, [tabindex]") ?? [])].filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled"),
  );
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

<button type="button" class="open-servers" data-testid="open-servers" aria-haspopup="dialog" onclick={() => void show()}>
  Servers
</button>

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
        {#if project}
          <label>
            Runs in
            <select bind:value={workDir} data-testid="servers-work-dir">
              <option value={null}>Main checkout</option>
              {#each workers as worker (worker.id)}
                <option value={worker.id}>{worker.name}</option>
              {/each}
            </select>
          </label>
        {/if}
        <button type="button" class="close" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      <div class="body">
        {#if !project}
          <p class="muted">Open a project to run its dev servers.</p>
        {:else if servers.unavailable}
          <p class="warn" role="note" data-testid="servers-unavailable">
            Servers cannot run here: they need the Linux sandbox, which keeps each work dir's servers in their own
            sandbox. ({servers.unavailable})
          </p>
        {:else}
          {#if servers.error}
            <p class="error" role="alert">{servers.error}</p>
          {/if}
          {#if list?.error}
            <p class="error" role="alert" data-testid="servers-file-error">
              Could not read <code>.ovra/servers.json</code>: {list.error}
            </p>
          {/if}
          {#if !list}
            <p class="muted">Loading…</p>
          {:else if list.definitions.length === 0 && !list.error}
            <div class="empty" data-testid="servers-empty">
              <p>No servers are defined for this work dir.</p>
              <p class="muted">
                Add them to <code>.ovra/servers.json</code> in the project. Each runs its command (no shell) in a sandbox
                with <code>PORT</code> set; agents of this work dir can reach it while it is ready. Only you can edit the
                file.
              </p>
              <pre>{example}</pre>
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
                    onclick={() => void servers.start(def.name)}>Start</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
          {#if list && list.servers.length > 0}
            <h3>Started</h3>
            <ul aria-label="Started servers">
              {#each [...list.servers].reverse() as server (server.id)}
                <li class="server" data-testid="server">
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
                        onclick={() => void servers.restart(server.id)}>Restart</button
                      >
                      <button
                        type="button"
                        aria-label={`Stop ${server.name}`}
                        disabled={!running(server.status) || server.status.state === "stopping"}
                        onclick={() => void servers.stop(server.id)}>Stop</button
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
  button:focus-visible,
  select:focus-visible,
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
    max-width: 900px;
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
</style>
