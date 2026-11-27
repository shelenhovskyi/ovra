<script lang="ts">
// Keys, a pane of Settings (docs/specs/secrets-consent.md): tokens agents use
// without seeing them. The netproxy adds one only to requests for its host,
// after the person allowed it for the task, or always by a UseKey rule. A
// key's value is typed here, sent to the core once and never shown again.
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import type { PolicyRule, SecretInfo } from "../lib/protocol";
import { ProjectPolicy, reason } from "../policy/policy.svelte";
import { projectSandbox } from "../settings/sandbox";
import { useOvra } from "../shell/context";
import { message, ProjectAccess } from "./access.svelte";
import { parseHost, reaches } from "./hosts";
import { type KeyPreset, OTHER, PRESETS } from "./presets";

type Consent = "ask" | "always" | "off";

const { transport, workspace } = useOvra();
const access = new ProjectAccess(transport);
const policy = new ProjectPolicy(transport);
const sessions = agentSessions(transport);

/** The key being added: from a preset or "Other API", until it is saved or cancelled. */
let adding = $state<{ preset: KeyPreset | null; secret: SecretInfo } | null>(null);
let value = $state("");
let error = $state<string | null>(null);
let busy = $state(false);
let root = $state<HTMLElement>();

const own = $derived(access.allowlist?.project.hosts ?? []);
const base = $derived(access.allowlist?.base.hosts ?? []);
const storeReady = $derived(access.store?.state === "available");
const sandbox = $derived(workspace.project ? projectSandbox(sessions.inProject(workspace.project.id)) : null);

$effect(() => () => access.dispose());

// Read the keys, and again for another project. Whatever was typed for one
// project is never sent for another.
$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => {
    cancel();
    void access.show(project);
    void policy.show(project);
    if (project) sessions.show(project).catch(() => {});
  });
});

function isKeyRule(rule: PolicyRule, name: string): boolean {
  return rule.tool === "UseKey" && rule.match === name;
}

/** What the project's UseKey rules say about `name`: deny wins, then ask, as in the core; a rule without a match is every key's. */
function consentOf(name: string): Consent {
  const rules = (policy.rules ?? []).filter((r) => r.tool === "UseKey" && (r.match === undefined || r.match === name));
  if (rules.some((r) => r.action === "deny")) return "off";
  if (rules.some((r) => r.action === "ask")) return "ask";
  return rules.some((r) => r.action === "auto" && r.match === name) ? "always" : "ask";
}

/** Replaces `name`'s UseKey rules: none asks, `auto` always allows, `deny` is off. */
async function setConsent(name: string, consent: Consent): Promise<void> {
  const current = policy.rules;
  if (!current) return;
  const kept = current.filter((r) => !isKeyRule(r, name));
  const rule = (action: "auto" | "deny"): PolicyRule => ({ tool: "UseKey", match: name, action });
  const next = consent === "ask" ? kept : [...kept, rule(consent === "always" ? "auto" : "deny")];
  try {
    await policy.save(next, current);
    error = null;
  } catch (err) {
    error = reason(err);
  }
}

async function start(preset: KeyPreset | null): Promise<void> {
  adding = { preset, secret: { ...(preset?.secret ?? OTHER) } };
  value = "";
  error = null;
  await tick();
  root?.querySelector<HTMLElement>("[data-testid=key-value], [data-testid=store-retry]")?.focus();
}

function cancel(): void {
  adding = null;
  value = "";
  error = null;
}

async function save(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!adding || busy) return;
  if (!value) {
    error = "Paste the key.";
    return;
  }
  const typed = adding.secret;
  const variable = typed.variable?.trim();
  const secret: SecretInfo = {
    name: typed.name.trim(),
    host: typed.host.trim().toLowerCase(),
    header: typed.header.trim(),
    prefix: typed.prefix,
    ...(variable ? { variable } : {}),
  };
  busy = true;
  try {
    await access.setSecret(secret, value);
    // The key goes nowhere its host is not allowed: allow it in the same step.
    const parsed = parseHost(secret.host);
    if (!("error" in parsed) && access.allowlist && !reaches([...base, ...own], secret.host)) {
      await access.setHosts([...own, parsed]);
    }
    cancel();
  } catch (err) {
    error = message(err);
  } finally {
    busy = false;
  }
}

async function remove(name: string): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    // The core drops the key's UseKey rules with it.
    await access.removeSecret(name);
    error = null;
  } catch (err) {
    error = message(err);
  } finally {
    busy = false;
  }
}
</script>

<div class="keys" data-testid="keys" bind:this={root}>
  {#if workspace.project}
    {#if access.error}
      <p class="error" role="alert">Could not read the project's keys: {access.error}</p>
    {/if}
    {#if sandbox === "none"}
      <p class="warn note" role="note" data-testid="keys-unsandboxed">
        Agents here run without a sandbox: Ovra adds no keys for them, and they could read your keychain like any
        program you run.
      </p>
    {/if}
    {#if access.secrets.length > 0}
      <ul aria-label="Keys">
        {#each access.secrets as secret (secret.name)}
          <li data-testid="key">
            <strong>{secret.name}</strong>
            <span class="muted"
              >for <code>{secret.host}</code>{#if secret.variable}, as <code>${secret.variable}</code>{/if}</span
            >
            {#if access.allowlist && !reaches([...base, ...own], secret.host)}
              <span class="warn">Agents cannot reach this host.</span>
            {/if}
            <label class="consent">
              <span class="sr-only">When agents use {secret.name}</span>
              <select
                value={consentOf(secret.name)}
                disabled={!policy.rules}
                data-testid="key-consent"
                onchange={(event) => void setConsent(secret.name, event.currentTarget.value as Consent)}
              >
                <option value="ask">Ask each task</option>
                <option value="always">Always allow</option>
                <option value="off">Off</option>
              </select>
            </label>
            <button type="button" aria-label={`Remove key ${secret.name}`} onclick={() => void remove(secret.name)}
              >Remove</button
            >
          </li>
        {/each}
      </ul>
    {:else if !access.loading && !access.error}
      <p class="muted" data-testid="keys-empty">No keys for this project yet.</p>
    {/if}

    {#if !adding}
      <div class="presets" role="group" aria-label="Add a key">
        {#each PRESETS as preset (preset.id)}
          <button type="button" data-preset={preset.id} onclick={() => void start(preset)}>{preset.label}</button>
        {/each}
        <button type="button" data-preset="other" onclick={() => void start(null)}>Other API…</button>
      </div>
    {:else if access.store?.state === "unavailable"}
      <div class="warn" role="note" data-testid="store-unavailable">
        <p>Keys can't be saved: the system keychain is not reachable. Ovra never keeps keys in a file.</p>
        <button type="button" data-testid="store-retry" onclick={() => void access.reload()}>
          {access.loading ? "Checking…" : "Check again"}
        </button>
        <button type="button" onclick={cancel}>Cancel</button>
        <details>
          <summary>Details</summary>
          <code data-testid="store-reason">{access.store.reason}</code>
        </details>
      </div>
    {:else}
      <form class="add" onsubmit={save} aria-label="Add a key">
        <h4>{adding.preset ? `${adding.preset.label} key` : "Key for another API"}</h4>
        {#if adding.preset}
          <p class="muted">
            {adding.preset.hint}
            <a
              href={adding.preset.url}
              onclick={(event) => {
                event.preventDefault();
                if (adding?.preset) void transport.call({ method: "open.url", params: { url: adding.preset.url } });
              }}>Make one</a
            >
          </p>
        {/if}
        <fieldset disabled={!storeReady}>
          <!-- Not type=password: browsers would offer to keep the key in their own password store. -->
          <label
            >Key <input
              class="token"
              bind:value
              placeholder="Paste it here"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              data-1p-ignore
              data-lpignore="true"
              data-testid="key-value"
            /></label
          >
          <details open={!adding.preset}>
            <summary>Advanced</summary>
            <div class="fields">
              <label>Name <input bind:value={adding.secret.name} placeholder="deploy" autocomplete="off" data-testid="key-name" /></label>
              <label
                >Host <input bind:value={adding.secret.host} placeholder="api.example.com" autocomplete="off" spellcheck="false" data-testid="key-host" /></label
              >
              <label>Header <input bind:value={adding.secret.header} autocomplete="off" spellcheck="false" /></label>
              <label>Prefix <input bind:value={adding.secret.prefix} autocomplete="off" spellcheck="false" /></label>
              <label
                >Variable <input
                  bind:value={adding.secret.variable}
                  placeholder="DEPLOY_TOKEN"
                  autocomplete="off"
                  spellcheck="false"
                  data-testid="key-variable"
                /></label
              >
            </div>
            <p class="muted">
              Agents see the variable with a stand-in, so their tools start; Ovra puts the real key in the header.
            </p>
          </details>
          <div class="buttons">
            <button type="submit" class="primary">Save key</button>
            <button type="button" onclick={cancel}>Cancel</button>
          </div>
        </fieldset>
        <p class="muted" data-testid={storeReady ? "store-available" : undefined}>
          {storeReady && access.store?.state === "available"
            ? `Kept in ${access.store.name}, never shown again. Ovra asks you before an agent uses it.`
            : "Checking the keychain…"}
        </p>
      </form>
    {/if}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
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
  input,
  select {
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
    background: var(--s4);
  }
  button.primary {
    color: var(--accent-ink);
    background: var(--accent);
  }
  button:disabled,
  input:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
  @media (forced-colors: active) {
    button,
    input,
    select {
      border: 1px solid ButtonText;
    }
  }
  .error {
    color: var(--danger);
    margin: 6px 0;
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
  div.warn p {
    margin: 0 0 6px;
  }
  div.warn code {
    display: block;
    margin-top: 4px;
    color: var(--text-2);
    overflow-wrap: anywhere;
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
  .consent {
    margin-left: auto;
  }
  li > button:last-child {
    padding: 2px 8px;
    color: var(--text-2);
    background: transparent;
  }
  .presets,
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 8px 0;
  }
  .add {
    margin: 8px 0;
    padding: 12px;
    border-radius: var(--radius);
    background: var(--s1);
  }
  h4 {
    margin: 0 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
    display: grid;
    gap: 8px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .fields {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
  .token {
    -webkit-text-security: disc;
  }
  summary {
    cursor: pointer;
    font-size: 13px;
    color: var(--text-2);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
