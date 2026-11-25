<script lang="ts">
// Network and secrets: a header button (and the "Network and secrets"
// command) opens a dialog with the hosts the open project's agents may reach
// and the tokens the netproxy adds for them. A token's value is typed here,
// sent to the core once and never shown again. Escape closes.
import { tick } from "svelte";
import { registerCommand } from "../keys/commands";
import type { AllowedHost } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { message, ProjectAccess } from "./access.svelte";
import { hostLabel, parseHost, reaches } from "./hosts";

const { transport, workspace } = useOvra();
const access = new ProjectAccess(transport);

let opened = $state(false);
let dialog = $state<HTMLElement>();
let returnTo: HTMLElement | null = null;

let newHost = $state("");
let hostError = $state<string | null>(null);
let name = $state("");
let host = $state("");
let header = $state("Authorization");
let prefix = $state("Bearer ");
let value = $state("");
let secretError = $state<string | null>(null);
let busy = $state(false);

const own = $derived(access.allowlist?.project.hosts ?? []);
const base = $derived(access.allowlist?.base.hosts ?? []);
const broad = $derived(new Set(access.allowlist?.broad ?? []));
const storeReady = $derived(access.store?.state === "available");
const unreachable = $derived(
  new Set(access.allowlist ? access.secrets.filter((s) => !reaches([...base, ...own], s.host)).map((s) => s.name) : []),
);

$effect(() => () => access.dispose());
$effect(() => registerCommand({ id: "network.toggle", title: "Network and secrets", run: toggle }));

$effect(() => {
  const project = workspace.project?.id ?? null;
  if (opened) void access.show(project);
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
  value = "";
  if (returnTo?.isConnected) returnTo.focus();
  returnTo = null;
}

function toggle(): void {
  if (opened) close();
  else void show();
}

async function saveHosts(hosts: AllowedHost[]): Promise<boolean> {
  // Buttons stay enabled while a call runs, so focus is never lost to the page.
  if (busy) return false;
  busy = true;
  try {
    await access.setHosts(hosts);
    hostError = null;
    return true;
  } catch (err) {
    hostError = message(err);
    return false;
  } finally {
    busy = false;
  }
}

async function addHost(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const parsed = parseHost(newHost);
  if ("error" in parsed) {
    hostError = parsed.error;
    return;
  }
  if (own.some((entry) => hostLabel(entry) === hostLabel(parsed))) {
    newHost = "";
    hostError = null;
    return;
  }
  if (await saveHosts([...own, parsed])) newHost = "";
}

function removeHost(entry: AllowedHost): void {
  void saveHosts(own.filter((other) => hostLabel(other) !== hostLabel(entry)));
}

function allow(target: string): void {
  const parsed = parseHost(target);
  if (!("error" in parsed)) void saveHosts([...own, parsed]);
}

async function addSecret(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!value) {
    secretError = "Paste the token's value.";
    return;
  }
  if (busy) return;
  busy = true;
  try {
    await access.setSecret(
      { name: name.trim(), host: host.trim().toLowerCase(), header: header.trim(), prefix },
      value,
    );
    value = "";
    name = "";
    host = "";
    secretError = null;
  } catch (err) {
    secretError = message(err);
  } finally {
    busy = false;
  }
}

async function removeSecret(secretName: string): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    await access.removeSecret(secretName);
    secretError = null;
  } catch (err) {
    secretError = message(err);
  } finally {
    busy = false;
  }
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
  const focusable = [...(dialog?.querySelectorAll<HTMLElement>("button, input, summary, [tabindex]") ?? [])].filter(
    (el) => el.tabIndex >= 0 && !(el as HTMLButtonElement).disabled,
  );
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
</script>

<button type="button" class="open-network" data-testid="open-network" aria-haspopup="dialog" onclick={() => void show()}>
  Network
</button>

{#if opened}
  <div class="backdrop">
    <div
      class="network"
      role="dialog"
      aria-modal="true"
      aria-labelledby="network-title"
      tabindex="-1"
      data-testid="network"
      bind:this={dialog}
      onkeydown={onDialogKeydown}
    >
      <header>
        <h2 id="network-title" tabindex="-1">Network and secrets</h2>
        <span class="muted">{workspace.project?.name ?? "No project"}</span>
        <button type="button" class="close" aria-keyshortcuts="Escape" onclick={close}>Close</button>
      </header>
      {#if !workspace.project}
        <p class="muted note">Open a project to see what its agents may reach.</p>
      {:else}
        {#if access.error}
          <p class="error" role="alert">Could not read the project's network settings: {access.error}</p>
        {/if}
        <div class="body">
          <section aria-labelledby="hosts-title">
            <h3 id="hosts-title">Allowed hosts</h3>
            <p class="muted">
              Sandboxed agents reach only these hosts, through Ovra's proxy. Everything else is refused and shows in the
              audit log.
            </p>
            {#if own.length === 0}
              <p class="muted" data-testid="hosts-empty">
                {access.loading && !access.allowlist ? "Loading…" : "No hosts added for this project."}
              </p>
            {:else}
              <ul aria-label="Project hosts">
                {#each own as entry (hostLabel(entry))}
                  <li data-testid="allowed-host">
                    <code>{hostLabel(entry)}</code>
                    {#if broad.has(entry.host)}
                      <span class="warn" data-testid="broad-host">
                        Anyone can receive data here, so an agent could send your files out through it.
                      </span>
                    {/if}
                    <button
                      type="button"
                      aria-label={`Remove ${hostLabel(entry)}`}
                     
                      onclick={() => removeHost(entry)}>Remove</button
                    >
                  </li>
                {/each}
              </ul>
            {/if}
            <form onsubmit={addHost}>
              <label>
                Add host
                <input
                  bind:value={newHost}
                  placeholder="registry.npmjs.org or host:8443"
                  autocomplete="off"
                  spellcheck="false"
                  data-testid="add-host"
                />
              </label>
              <button type="submit">Allow</button>
            </form>
            {#if hostError}
              <p class="error" role="alert">{hostError}</p>
            {/if}
            {#if base.length > 0}
              <details>
                <summary>Always allowed for the agent itself ({base.length})</summary>
                <ul aria-label="Always allowed">
                  {#each base as entry (hostLabel(entry))}
                    <li><code>{hostLabel(entry)}</code></li>
                  {/each}
                </ul>
              </details>
            {/if}
          </section>

          <section aria-labelledby="secrets-title">
            <h3 id="secrets-title">Secrets</h3>
            <p class="muted">
              The proxy adds a token to requests for one host, so agents use it without ever seeing it. Only sandboxed
              sessions get secrets. An agent can still do anything the token allows on that host, so give it the
              narrowest scope you can.
            </p>
            {#if access.store?.state === "unavailable"}
              <p class="warn" data-testid="store-unavailable">
                Secrets cannot be stored: {access.store.reason}. Ovra keeps them only in the system keychain, never in a
                file.
              </p>
            {:else if access.store?.state === "available"}
              <p class="muted" data-testid="store-available">Values are kept in {access.store.name}.</p>
            {/if}
            {#if access.secrets.length > 0}
              <ul aria-label="Secrets">
                {#each access.secrets as secret (secret.name)}
                  <li data-testid="secret">
                    <strong>{secret.name}</strong>
                    <span class="muted"><code>{secret.header}</code> for <code>{secret.host}</code></span>
                    {#if unreachable.has(secret.name)}
                      <span class="warn">
                        Agents cannot reach this host yet.
                        <button type="button" onclick={() => allow(secret.host)}>Allow it</button>
                      </span>
                    {/if}
                    <button
                      type="button"
                      aria-label={`Remove secret ${secret.name}`}
                     
                      onclick={() => void removeSecret(secret.name)}>Remove</button
                    >
                  </li>
                {/each}
              </ul>
            {/if}
            <form class="secret-form" onsubmit={addSecret} aria-label="Add a secret">
              <fieldset disabled={!storeReady}>
                <label>Name <input bind:value={name} placeholder="github" autocomplete="off" data-testid="secret-name" /></label>
                <label>Host <input bind:value={host} placeholder="api.github.com" autocomplete="off" spellcheck="false" data-testid="secret-host" /></label>
                <label>Header <input bind:value={header} autocomplete="off" spellcheck="false" /></label>
                <label>Prefix <input bind:value={prefix} autocomplete="off" spellcheck="false" /></label>
                <label
                  >Value <input
                    type="password"
                    bind:value
                    autocomplete="off"
                    spellcheck="false"
                    data-testid="secret-value"
                  /></label
                >
                <button type="submit">Save secret</button>
              </fieldset>
            </form>
            <p class="muted">
              The value goes to the keychain and is never shown again. Saving under an existing name replaces it.
            </p>
            {#if secretError}
              <p class="error" role="alert">{secretError}</p>
            {/if}
          </section>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  button,
  input {
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
  input:focus-visible,
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
  .network {
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
    margin: 0 0 4px;
    font-size: 13px;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 0 12px 12px;
  }
  section {
    padding: 10px 0;
    border-bottom: 1px solid #24242b;
  }
  .muted {
    color: #b4b4c0;
    font-size: 12px;
  }
  .note {
    margin: 6px 12px;
  }
  .error {
    color: #ff8f8f;
    margin: 6px 0;
  }
  .warn {
    color: #ffd36b;
    font-size: 12px;
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
  li > button:last-child {
    margin-left: auto;
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
    margin: 6px 0;
  }
  fieldset {
    display: contents;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: #c8c8d2;
  }
  details {
    margin-top: 6px;
    font-size: 12px;
    color: #b4b4c0;
  }
</style>
