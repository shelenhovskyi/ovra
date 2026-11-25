<script lang="ts">
// The Servers tab of the bottom dock, next to the terminals: the dev servers
// that run (or ran) for the open project's main checkout or a worker's
// worktree, with their output. "Show servers" (Alt+Mod+S) shows it, goes
// into it, and hides it from inside; Escape inside hides it too. Settings >
// Servers defines and starts them.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import type { DevServerId as ServerId, DevServerInfo as ServerInfo, StartedBy } from "../lib/protocol";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import Icon from "../shell/Icon.svelte";
import { serversDock } from "./dock.svelte";
import LogView from "./LogView.svelte";
import { exitDetail, ProjectServers, running } from "./servers.svelte";

const { transport, workspace } = useOvra();
const servers = new ProjectServers(transport);
const sessions = agentSessions(transport);

const KEYS = "Mod+Alt+S";
type Action = "Stop" | "Restart";

let panel = $state<HTMLElement>();
let logsOf = $state(new Set<string>());
let returnTo: HTMLElement | null = null;
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
const started = $derived([...(servers.list?.servers ?? [])].reverse());

$effect(() => () => servers.dispose());
$effect(() =>
  registerCommand({
    id: "servers.toggle",
    title: "Show servers",
    keys: KEYS,
    category: "View",
    keywords: ["dev servers"],
    run: toggle,
  }),
);

// Follow the project and the work dir the tab shows, also while it is hidden: the status bar counts what runs.
$effect(() => {
  const shown = { project, workDir: serversDock.workDir };
  untrack(() => {
    void servers.show(shown.project, shown.workDir);
    if (shown.project) sessions.load(shown.project).catch(() => {});
  });
});

// Another project has other workers.
$effect(() => {
  void project;
  untrack(() => {
    serversDock.workDir = null;
    logsOf = new Set();
  });
});

$effect(() => {
  if (serversDock.started > 0) untrack(() => void servers.reload());
});

$effect(() => {
  serversDock.running = started.filter((s) => running(s.status)).length;
});

/** Runs whose failure opened their output, so a person who closes it again keeps it closed. */
const failureShown = new Set<string>();
// A run that failed shows its output at once: the reason is in its last lines.
$effect(() => {
  const failed = started.filter((s) => servers.failed(s)).map((s) => [s.id, `${s.id}:${s.generation}`] as const);
  untrack(() => {
    const fresh = failed.filter(([, run]) => !failureShown.has(run));
    if (fresh.length === 0) return;
    for (const [, run] of fresh) failureShown.add(run);
    logsOf = new Set([...logsOf, ...fresh.map(([id]) => id)]);
  });
});

// A status change can disable the focused button: focus moves on to the row's next one.
$effect(() => {
  void servers.list;
  if (serversDock.shown) void tick().then(placeFocus);
});

function heading(): HTMLElement | null {
  return panel?.querySelector<HTMLElement>("h2") ?? null;
}

async function show(): Promise<void> {
  const active = document.activeElement;
  if (!panel?.contains(active)) returnTo = active instanceof HTMLElement ? active : null;
  serversDock.shown = true;
  await tick();
  heading()?.focus();
}

function hide(): void {
  const inside = !!panel?.contains(document.activeElement);
  serversDock.shown = false;
  keep = null;
  standIn = null;
  if (inside && returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (serversDock.shown && panel?.contains(document.activeElement)) hide();
  else void show();
}

/** Moves focus off a disabled or vanished button to the kept row's preferred enabled one, else its Output. */
function placeFocus(): void {
  if (!keep || !panel) return;
  const active = document.activeElement;
  const lost =
    !(active instanceof HTMLElement) || !panel.contains(active) || active.matches(":disabled") || active === standIn;
  if (!lost) return;
  const buttons = [...panel.querySelectorAll<HTMLButtonElement>(`[data-server="${keep.row}"] .actions button`)];
  const enabled = (name: string) => buttons.find((b) => b.textContent?.trim() === name && !b.disabled);
  const best = keep.prefer.map(enabled).find((b) => b !== undefined);
  const target = best ?? enabled("Output") ?? heading();
  target?.focus();
  standIn = best ? null : target;
}

async function act(row: ServerId, prefer: Action[], action: () => Promise<ServerInfo | null>) {
  keep = { row, prefer };
  const now = await action();
  if (now) keep = { row: now.id, prefer };
  await tick();
  placeFocus();
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

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape" || event.defaultPrevented) return;
  event.preventDefault();
  hide();
}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="servers-dock"
  aria-labelledby="servers-dock-title"
  hidden={!serversDock.shown}
  data-testid="servers"
  bind:this={panel}
  {onkeydown}
>
  <div class="bar">
    <h2 id="servers-dock-title" tabindex="-1">Servers</h2>
    {#if project && (workers.length > 0 || serversDock.workDir !== null)}
      <!-- Only a choice once a worker has its own copy of the project. -->
      <label>
        Runs in
        <select bind:value={serversDock.workDir} data-testid="servers-work-dir">
          <option value={null}>Project folder</option>
          {#each workers as worker (worker.id)}
            <option value={worker.id}>{worker.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    <button type="button" class="hide" aria-label="Hide servers" title="Hide servers (Escape)" onclick={hide}
      ><Icon name="chevron" small /></button
    >
  </div>
  <div class="body">
    {#if servers.unavailable}
      <p class="warn" role="note" data-testid="servers-unavailable">
        Servers cannot run here: they need the Linux sandbox. ({servers.unavailable})
      </p>
    {:else if servers.error}
      <p class="error" role="alert">{servers.error}</p>
    {/if}
    {#if project && servers.list && started.length === 0 && !servers.unavailable}
      <EmptyState
        headline="No servers running"
        line="Start a server from Settings > Servers; agents can start them too."
        actions={[{ label: "Server settings", run: () => openSettings("servers") }]}
      />
    {:else if started.length > 0}
      <ul aria-label="Started servers">
        {#each started as server (server.id)}
          <li class="server" data-testid="server" data-server={server.id}>
            <div class="row">
              <strong>{server.name}</strong>
              <span class={`badge ${server.status.state}`} data-testid="server-status" title={exitDetail(server.status)}
                >{servers.statusOf(server)}</span
              >
              <span class="muted">port {server.port}</span>
              <span class="muted">in {where(server)}</span>
              <span class="muted">started by {who(server.started_by)}</span>
              <span class="muted">run {server.generation}</span>
              <span class="actions">
                <button type="button" aria-expanded={logsOf.has(server.id)} onclick={() => toggleLogs(server.id)}
                  >Output</button
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
  </div>
</section>

<style>
  .servers-dock {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--s2);
  }
  .servers-dock[hidden] {
    display: none;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 34px;
    padding: 4px 8px 4px 12px;
  }
  h2 {
    margin: 0;
    font: var(--t-headline);
  }
  label,
  .muted {
    font: var(--t-small);
    color: var(--text-2);
  }
  button,
  select {
    font: inherit;
    padding: 2px 8px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  button:disabled {
    opacity: 0.55;
  }
  .hide {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-left: auto;
    padding: 0;
    color: var(--text-2);
    background: none;
  }
  .hide:hover {
    color: var(--text);
    background: var(--s3);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 0 12px 12px;
    overflow: auto;
  }
  ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    padding: 6px 10px;
    border-radius: var(--r-md);
    background: var(--s1);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .actions {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }
  .actions button {
    color: var(--text-2);
    background: transparent;
  }
  .actions button:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .badge {
    padding: 1px 8px;
    font: var(--t-caption);
    border-radius: var(--r-sm);
    background: var(--s3);
  }
  .ready {
    color: var(--ok);
    background: var(--ok-soft);
  }
  .starting,
  .stopping {
    color: var(--warn);
    background: var(--warn-soft);
  }
  .failed,
  .error {
    color: var(--danger);
  }
  .warn {
    color: var(--warn);
  }
</style>
