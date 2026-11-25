<script lang="ts">
// Network and secrets: a header button (and the "Network and secrets"
// command) opens a dialog with the hosts the open project's agents may reach
// and the tokens the netproxy adds for them. A token's value is typed here,
// sent to the core once and never shown again. Escape closes.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { registerCommand } from "../keys/commands";
import type { AllowedHost, SessionInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { message, ProjectAccess } from "./access.svelte";
import { hostLabel, parseHost, reaches } from "./hosts";

const { transport, workspace } = useOvra();
const access = new ProjectAccess(transport);
const sessions = agentSessions(transport);

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
// Sessions this page does not protect: the allowlist does not limit them and
// they can read the keychain like any program the person runs.
const exposed = $derived(workspace.project ? sessions.inProject(workspace.project.id).filter(unprotected) : []);
// Without a sandbox a session can also read the keychain like any program the person runs.
const bare = $derived(exposed.filter((s) => s.sandbox === null).length);
// What the sandbox of the project's live sessions does not cover, each once.
const gaps = $derived(
  workspace.project
    ? [
        ...new Set(
          sessions
            .inProject(workspace.project.id)
            .filter(live)
            .flatMap((s) => s.sandbox?.gaps ?? []),
        ),
      ]
    : [],
);
let sessionsError = $state(false);
const unreachable = $derived(
  new Set(access.allowlist ? access.secrets.filter((s) => !reaches([...base, ...own], s.host)).map((s) => s.name) : []),
);

$effect(() => () => access.dispose());
$effect(() => registerCommand({ id: "network.toggle", title: "Network and secrets", run: toggle }));

// Read the settings when the dialog opens, and again for another project.
// Whatever was typed for one project is never sent for another.
$effect(() => {
  const project = workspace.project?.id ?? null;
  if (!opened) return;
  untrack(() => {
    clearForm();
    void access.show(project);
    sessionsError = false;
    if (project)
      sessions.load(project).catch(() => {
        sessionsError = true;
      });
  });
});

function live(session: SessionInfo): boolean {
  return session.status.state !== "stopped" && session.status.state !== "failed";
}

function unprotected(session: SessionInfo): boolean {
  if (!live(session)) return false;
  return session.sandbox === null || session.sandbox.network === "open";
}

function clearForm(): void {
  newHost = "";
  hostError = null;
  name = "";
  host = "";
  value = "";
  secretError = null;
}

async function show(): Promise<void> {
  const active = document.activeElement;
  returnTo = active instanceof HTMLElement ? active : null;
  opened = true;
  await tick();
  dialog?.querySelector<HTMLElement>("h2")?.focus();
}

function close(): void {
  opened = false;
  clearForm();
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
  // Keep Tab inside the dialog (aria-modal), also from the heading.
  const focusable = [...(dialog?.querySelectorAll<HTMLElement>("button, input, summary, [tabindex]") ?? [])].filter(
    (el) => el.tabIndex >= 0 && !el.matches(":disabled"),
  );
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (!first || !last) return;
  const at = focusable.indexOf(document.activeElement as HTMLElement);
  if (at < 0) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  } else if (event.shiftKey && at === 0) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && at === focusable.length - 1) {
    event.preventDefault();
    first.focus();
  }
}
</script>

<button type="button" class="open-network" data-testid="open-network" aria-haspopup="dialog" onclick={() => void show()}>
  Network
</button>

{#if opened}
  <!-- A click beside the dialog must not take focus out of it. -->
  <div class="backdrop" role="presentation" onmousedown={(event) => event.target === event.currentTarget && event.preventDefault()}>
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
        {#if exposed.length > 0}
          <p class="warn note" role="note" data-testid="unsandboxed">
            {exposed.length === 1 ? "One session of this project runs" : `${exposed.length} sessions of this project run`}
            {bare > 0 ? "without Ovra's sandbox" : "with an open network"}. These hosts do not limit
            {exposed.length === 1 ? "it" : "them"}{bare > 0
              ? ", and a session without the sandbox can read stored secrets from the keychain like any program you run"
              : ""}.
          </p>
        {:else if sessionsError}
          <p class="warn note" role="note" data-testid="unsandboxed">
            Could not check whether this project's sessions run in the sandbox.
          </p>
        {/if}
        <div class="body">
          <section aria-labelledby="hosts-title">
            <h3 id="hosts-title">Allowed hosts</h3>
            <p class="muted">
              Sandboxed agents reach only these hosts, through Ovra's proxy. Everything else is refused and shows in the
              audit log.
            </p>
            {#if gaps.length > 0}
              <p class="warn" data-testid="sandbox-gaps">Not covered: {gaps.join(", ")}.</p>
            {/if}
            {#if own.length === 0 && !access.error}
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
              <!-- The core replaces the whole list, so adding waits until it is known. -->
              <fieldset disabled={!access.allowlist}>
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
              </fieldset>
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
                <!-- Not type=password: browsers would offer to keep the token in their own password store. -->
                <label
                  >Value <input
                    class="token"
                    bind:value
                    autocomplete="off"
                    autocapitalize="off"
                    spellcheck="false"
                    aria-describedby="secret-value-note"
                    data-1p-ignore
                    data-lpignore="true"
                    data-testid="secret-value"
                  /></label
                >
                <button type="submit">Save secret</button>
              </fieldset>
            </form>
            <p class="muted" id="secret-value-note">
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
  .token {
    -webkit-text-security: disc;
  }
  details {
    margin-top: 6px;
    font-size: 12px;
    color: #b4b4c0;
  }
</style>
