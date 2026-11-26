<script lang="ts">
// Network and secrets, the Network pane of Settings: the hosts the open
// project's agents may reach and the tokens the netproxy adds for them. A
// token's value is typed here, sent to the core once and never shown again.
import { tick, untrack } from "svelte";
import { NO_SANDBOX, sandboxState } from "../agents/sandbox";
import { agentSessions } from "../agents/sessions.svelte";
import type { AllowedHost, SessionInfo } from "../lib/protocol";
import { projectSandbox } from "../settings/sandbox";
import { useOvra } from "../shell/context";
import { message, ProjectAccess } from "./access.svelte";
import { hostLabel, parseHost, reaches } from "./hosts";

const { transport, workspace } = useOvra();
const access = new ProjectAccess(transport);
const sessions = agentSessions(transport);

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
const bare = $derived(exposed.filter((s) => sandboxState(s).kind === "none").length);
/** How agents run here, as the session says it: also known after they ended. */
const sandbox = $derived(workspace.project ? projectSandbox(sessions.inProject(workspace.project.id)) : null);
/** The person is adding a secret: only then the form, and whether the keychain can keep it. */
let addingSecret = $state(false);
let root = $state<HTMLElement>();

/** The pressed button goes: focus moves on to the form's first field, or to Check again without a keychain. */
async function openSecretForm(): Promise<void> {
  addingSecret = true;
  await tick();
  root?.querySelector<HTMLElement>("[data-testid=secret-name], [data-testid=store-retry]")?.focus();
}
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

// Read the settings, and again for another project. Whatever was typed for
// one project is never sent for another.
$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => {
    clearForm();
    void access.show(project);
    sessionsError = false;
    if (project)
      sessions.show(project).catch(() => {
        sessionsError = true;
      });
  });
});

function live(session: SessionInfo): boolean {
  return session.status.state !== "stopped" && session.status.state !== "failed";
}

function unprotected(session: SessionInfo): boolean {
  // A session that has not said how it runs yet is neither: the note waits for its report.
  if (!live(session)) return false;
  const state = sandboxState(session);
  return state.kind === "none" || (state.kind === "sandboxed" && state.report.network === "open");
}

function clearForm(): void {
  addingSecret = false;
  newHost = "";
  hostError = null;
  name = "";
  host = "";
  value = "";
  secretError = null;
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
</script>

<div class="network" data-testid="network" bind:this={root}>
  {#if workspace.project}
    {#if access.error}
      <p class="error" role="alert">Could not read the project's network settings: {access.error}</p>
    {/if}
    <!-- The session's own words for no sandbox, and what that means here, in one line. -->
    {#if bare > 0 || sandbox === "none"}
      <p class="warn note" role="note" data-testid="unsandboxed">
        {NO_SANDBOX}: these hosts don't limit it, and it can read stored secrets.
      </p>
    {:else if exposed.length > 0}
      <p class="warn note" role="note" data-testid="unsandboxed">
        Network not limited: these hosts don't limit {exposed.length === 1 ? "one agent" : `${exposed.length} agents`}
        here.
      </p>
    {:else if sessionsError}
      <p class="warn note" role="note" data-testid="unsandboxed">Could not check whether agents here run in a sandbox.</p>
    {/if}
    <div class="body">
      <section aria-labelledby="hosts-title">
        <h3 id="hosts-title">Allowed hosts</h3>
        <p class="muted">Sandboxed agents reach only these; the rest is refused and logged.</p>
        {#if gaps.length > 0}
          <p class="muted" data-testid="sandbox-gaps">The sandbox also does not cover: {gaps.join(", ")}.</p>
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
                    Anyone can receive data here: an agent could send your files out.
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
          <button type="submit" class="primary">Allow</button>
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
        <p class="muted">The proxy adds a token for one host; sandboxed agents use it without seeing it. Keep its scope narrow.</p>
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
        {#if !addingSecret}
          <button type="button" data-testid="add-secret" onclick={() => void openSecretForm()}>Add secret…</button>
        {:else if access.store?.state === "unavailable"}
          <div class="warn" role="note" data-testid="store-unavailable">
            <p>
              Secrets can't be saved: the system keychain is not reachable. Ovra never keeps secrets in a file.
            </p>
            <button type="button" data-testid="store-retry" onclick={() => void access.reload()}>
              {access.loading ? "Checking…" : "Check again"}
            </button>
            <details>
              <summary>Details</summary>
              <code data-testid="store-reason">{access.store.reason}</code>
            </details>
          </div>
        {:else}
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
            <button type="submit" class="primary" aria-describedby={storeReady ? undefined : "secret-off"}>Save secret</button>
            {#if !storeReady}
              <span class="muted off" id="secret-off" data-testid="secret-off">Checking the keychain…</span>
            {/if}
          </fieldset>
        </form>
        <p class="muted" id="secret-value-note" data-testid={storeReady ? "store-available" : undefined}>
          Kept in {access.store?.state === "available" ? access.store.name : "the keychain"}, never shown again; the
          same name replaces it.
        </p>
        {/if}
        {#if secretError}
          <p class="error" role="alert">{secretError}</p>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .muted,
  label {
    color: var(--text-2);
    font-size: 13px;
  }
  .note {
    margin: 0 0 8px;
  }
  button,
  input {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  button {
    font-weight: 500;
    cursor: pointer;
  }
  button:hover {
    color: var(--text);
    background: var(--s4);
  }
  button.primary {
    color: var(--accent-ink);
    background: var(--accent);
  }
  button:disabled,
  input:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
  summary {
    cursor: pointer;
  }
  .error {
    color: var(--danger);
    margin: 6px 0;
  }
  @media (forced-colors: active) {
    button,
    input {
      border: 1px solid ButtonText;
    }
  }
  h3 {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 600;
  }
  section {
    padding: 12px 0;
  }
  section + section {
    border-top: 0;
    margin-top: 8px;
  }
  .warn {
    color: var(--warn);
    font-size: 13px;
  }
  p.warn,
  div.warn {
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--warn-soft);
  }
  ul {
    list-style: none;
    display: grid;
    gap: 4px;
    margin: 8px 0;
    padding: 0;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--radius);
    background: var(--s1);
  }
  li > button:last-child {
    margin-left: auto;
    padding: 2px 8px;
    color: var(--text-2);
    background: transparent;
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 8px;
    margin: 8px 0;
  }
  fieldset {
    display: contents;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .token {
    -webkit-text-security: disc;
  }
  div.warn p {
    margin: 0 0 6px;
  }
  div.warn code {
    display: block;
    margin-top: 4px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .off {
    align-self: center;
  }
  details {
    margin-top: 6px;
    font-size: 13px;
    color: var(--text-2);
  }
</style>
