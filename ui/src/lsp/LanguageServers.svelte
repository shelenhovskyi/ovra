<script lang="ts">
// Status-bar item for the open project's language servers (docs/specs/lsp.md):
// hidden while there is nothing to say, otherwise a button that opens a small
// panel with each server, whether it is sandboxed, why it failed, a Stop
// button, and the servers that are not installed. Escape closes it.
import { tick, untrack } from "svelte";
import { serverLanguageFor } from "../editor/lsp/clients";
import { focusMain } from "../editor/open-files.svelte";
import { listCommands } from "../keys/commands";
import { runCommand } from "../keys/keybindings";
import type { ServerInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { languageServers } from "./servers.svelte";

const { transport, workspace } = useOvra();
const lsp = languageServers(transport);

let opened = $state(false);
let button = $state<HTMLButtonElement>();
let panel = $state<HTMLElement>();
let stopError = $state<string | null>(null);
let stopped = $state<string | null>(null);

const servers = $derived(lsp.servers);
const untrusted = $derived(lsp.status?.trusted === false);
// The shown file's language decides what the item names.
const shownLanguage = $derived(workspace.openPath ? serverLanguageFor(workspace.openPath) : null);
// Without a file of a served language on screen, a missing server counts while none runs.
const shownHint = $derived(
  shownLanguage ? lsp.hints.find((k) => k.language === shownLanguage) : servers.length === 0 ? lsp.hints[0] : undefined,
);
const shownServer = $derived(servers.find((s) => s.language === shownLanguage));
const attention = $derived(servers.some((s) => s.sandbox === null || s.state === "failed") || shownHint !== undefined);
const visible = $derived(servers.length > 0 || lsp.hints.length > 0 || (untrusted && lsp.needed.size > 0));
const label = $derived.by(() => {
  if (untrusted) return "Code intelligence: off";
  if (shownHint) return `${shownHint.name} not installed`;
  if (shownServer) return servers.length > 1 ? `${shownServer.name} +${servers.length - 1}` : shownServer.name;
  if (servers.length === 1) return servers[0]?.name ?? "";
  if (servers.length > 1) return `${servers.length} language servers`;
  return "Language servers";
});
// Needed ones first: those are what the person just ran into.
const missing = $derived([...lsp.hints, ...lsp.missing.filter((k) => !lsp.hints.includes(k))]);

// Again when trust changes: it decides whether servers may start.
$effect(() => {
  const project = workspace.project?.id ?? null;
  void workspace.project?.trusted;
  untrack(() => void lsp.show(project));
});

$effect(() => {
  if (!visible && opened) opened = false;
});

function stateText(server: ServerInfo): string {
  if (server.state === "starting") return "Starting";
  if (server.state === "running") return "Running";
  if (server.state === "failed") return "Failed";
  return "Stopped";
}

async function toggle(): Promise<void> {
  if (opened) {
    close();
    return;
  }
  opened = true;
  stopError = null;
  stopped = null;
  await tick();
  panel?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
  button?.focus();
}

async function stop(server: ServerInfo): Promise<void> {
  try {
    await lsp.stop(server.id);
    stopError = null;
    stopped = `Stopped ${server.name}. It starts again the next time you open a ${server.language} file.`;
  } catch (err) {
    stopError = `Could not stop ${server.name}: ${err instanceof Error ? err.message : String(err)}`;
  }
  // The item goes away with its last server: focus goes back to the editor then.
  await tick();
  if (panel?.isConnected) panel.querySelector<HTMLElement>("h2")?.focus();
  else if (button?.isConnected) button.focus();
  else await focusMain();
}

/** The command that opens the project's trust card, while its panel is mounted. */
function trustCommand() {
  return listCommands().find((c) => c.id === "projects.review_trust");
}

function reviewTrust(): void {
  const command = trustCommand();
  opened = false;
  if (command) void runCommand(command);
}

/** Closes the panel when focus or a click goes elsewhere, without taking focus back. */
function onFocusout(event: FocusEvent): void {
  const next = event.relatedTarget;
  if (next instanceof Node && (panel?.contains(next) || button?.contains(next))) return;
  if (next) opened = false;
}

$effect(() => {
  if (!opened) return;
  const away = (event: PointerEvent) => {
    const target = event.target;
    if (target instanceof Node && !panel?.contains(target) && !button?.contains(target)) opened = false;
  };
  window.addEventListener("pointerdown", away, true);
  return () => window.removeEventListener("pointerdown", away, true);
});

function onKeydown(event: KeyboardEvent): void {
  if (event.code !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  close();
}
</script>

{#if visible}
  <div class="lsp">
    <button
      type="button"
      class="item"
      data-testid="lsp-status"
      aria-expanded={opened}
      aria-controls="lsp-panel"
      aria-label={attention ? `${label}, needs attention` : label}
      bind:this={button}
      onclick={() => void toggle()}
    >
      {#if attention}<span class="mark" aria-hidden="true">!</span>{/if}{label}
    </button>
    <!-- Trusting the project is what turns the servers on. -->
    {#if untrusted && trustCommand()}
      <button type="button" class="item turn-on" data-testid="lsp-turn-on" onclick={reviewTrust}>Turn on</button>
    {/if}
    {#if opened}
      <div
        id="lsp-panel"
        class="panel"
        role="dialog"
        aria-labelledby="lsp-title"
        tabindex="-1"
        data-testid="lsp-panel"
        bind:this={panel}
        onkeydown={onKeydown}
        onfocusout={onFocusout}
      >
        <h2 id="lsp-title" tabindex="-1">Language servers</h2>
        {#if untrusted}
          <p class="muted" data-testid="lsp-untrusted">
            Language servers run this project's code, so they start only in trusted projects.
          </p>
          {#if trustCommand()}
            <button type="button" onclick={reviewTrust}>Review and trust…</button>
          {/if}
        {/if}
        {#if lsp.error}
          <p class="error" role="alert">Could not read the language servers: {lsp.error}</p>
        {/if}
        {#if stopError}
          <p class="error" role="alert">{stopError}</p>
        {/if}
        {#if stopped}
          <p class="muted" role="status">{stopped}</p>
        {/if}
        {#if servers.length > 0}
          <ul aria-label="Running language servers">
            {#each servers as server (server.id)}
              <li data-testid="lsp-server">
                <div class="row">
                  <strong>{server.name}</strong>
                  <span class="muted">{server.language}</span>
                  <span class="muted">{stateText(server)}</span>
                  {#if server.state === "starting" || server.state === "running"}
                    <button type="button" aria-label={`Stop ${server.name}`} onclick={() => void stop(server)}>Stop</button>
                  {/if}
                </div>
                {#if server.sandbox === null}
                  <p class="warn" data-testid="lsp-unsandboxed">
                    Runs without a sandbox: it can read and change your files and reach the network.
                  </p>
                {:else}
                  <p class="muted">
                    In the sandbox: reads the project, writes only its own cache, no network ({server.sandbox}).
                  </p>
                {/if}
                {#if server.state === "failed"}
                  <p class="error" data-testid="lsp-reason">{server.reason}</p>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#if missing.length > 0}
          <h3>Not installed</h3>
          <p class="muted">Ovra does not install language servers. To get one, run its install command yourself.</p>
          <ul aria-label="Not installed">
            {#each missing as known (known.language)}
              <li data-testid="lsp-missing">
                <div class="row">
                  <strong>{known.name}</strong>
                  <span class="muted">{known.language}</span>
                </div>
                <code>{known.install}</code>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .lsp {
    position: relative;
    margin-left: auto;
  }
  button {
    font: inherit;
    padding: 2px 10px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  .item {
    background: none;
    color: inherit;
  }
  .item:hover {
    background: var(--s3);
  }
  .turn-on {
    color: var(--accent);
  }
  h2:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .mark {
    margin-right: 4px;
    font-weight: 600;
    color: var(--warn);
  }
  .panel {
    position: absolute;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 80;
    width: 380px;
    max-height: 60vh;
    overflow: auto;
    padding: 12px 14px;
    background: var(--s4);
    border-radius: var(--radius);
    box-shadow: var(--card-shadow);
    color: var(--text);
    font-size: 13px;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 600;
  }
  h3 {
    margin: 12px 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 4px 0;
    padding: 0;
  }
  li {
    padding: 6px 0;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .row strong {
    font-weight: 500;
  }
  .row button {
    margin-left: auto;
  }
  p {
    margin: 2px 0;
  }
  .muted {
    color: var(--text-2);
  }
  .warn {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  code {
    display: block;
    margin-top: 4px;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--s3);
    font-size: 12px;
    user-select: all;
    overflow-wrap: anywhere;
  }
  @media (forced-colors: active) {
    .panel {
      border: 1px solid CanvasText;
    }
  }
</style>
