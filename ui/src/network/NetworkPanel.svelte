<script lang="ts">
// The Network pane of Settings: the hosts the open project's agents may
// reach. Keys the netproxy adds for them have their own pane (KeysPane).
import { untrack } from "svelte";
import { NO_SANDBOX, sandboxState } from "../agents/sandbox";
import { agentSessions } from "../agents/sessions.svelte";
import type { AllowedHost, SessionInfo } from "../lib/protocol";
import { projectSandbox } from "../settings/sandbox";
import { useOvra } from "../shell/context";
import { message, ProjectAccess } from "./access.svelte";
import { hostLabel, parseHost } from "./hosts";

const { transport, workspace } = useOvra();
const access = new ProjectAccess(transport);
const sessions = agentSessions(transport);

let newHost = $state("");
let hostError = $state<string | null>(null);
let busy = $state(false);

const own = $derived(access.allowlist?.project.hosts ?? []);
const base = $derived(access.allowlist?.base.hosts ?? []);
const broad = $derived(new Set(access.allowlist?.broad ?? []));
// Sessions this page does not protect: the allowlist does not limit them and
// they can read the keychain like any program the person runs.
const exposed = $derived(workspace.project ? sessions.inProject(workspace.project.id).filter(unprotected) : []);
// Without a sandbox a session can also read the keychain like any program the person runs.
const bare = $derived(exposed.filter((s) => sandboxState(s).kind === "none").length);
/** How agents run here, as the session says it: also known after they ended. */
const sandbox = $derived(workspace.project ? projectSandbox(sessions.inProject(workspace.project.id)) : null);
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
  newHost = "";
  hostError = null;
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
</script>

<div class="network" data-testid="network">
  {#if workspace.project}
    {#if access.error}
      <p class="error" role="alert">Could not read the project's network settings: {access.error}</p>
    {/if}
    <!-- The session's own words for no sandbox, and what that means here, in one line. -->
    {#if bare > 0 || sandbox === "none"}
      <p class="warn note" role="note" data-testid="unsandboxed">
        {NO_SANDBOX}: these hosts don't limit it.
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
  .warn {
    color: var(--warn);
    font-size: 13px;
  }
  p.warn {
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
  details {
    margin-top: 6px;
    font-size: 13px;
    color: var(--text-2);
  }
</style>
