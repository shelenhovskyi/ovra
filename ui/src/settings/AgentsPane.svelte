<script lang="ts">
// Claude in plain facts, for Settings > Agents and the start screen: when
// Claude is ready, who pays, the version and the program that runs, with
// quiet ways to check again or choose another path; repair (install, sign
// in, find the program) only when something is wrong. A billing warning
// (an API key in the environment) always stays on top; the setup card says
// it itself when something is wrong.
import { untrack } from "svelte";
import { providers } from "../agents/providers.svelte";
import Accounts from "../agents/setup/Accounts.svelte";
import SetupCard from "../agents/setup/AgentSetup.svelte";
import { advice, billingText } from "../agents/setup/claude-texts";
import type { AgentSetup } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

const { transport } = useOvra();
const names = providers(transport);

let setup = $state<AgentSetup | null>(null);
let error = $state<string | null>(null);
let busy = $state(false);
let editing = $state(false);
let path = $state("");
let pathError = $state<string | null>(null);

const ready = $derived(setup?.state.state === "ready");
const billing = $derived(setup ? billingText(setup) : null);

$effect(() => untrack(() => void check()));

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function check(): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    setup = (await transport.call({ method: "agents.setup", params: { agent: "claude" } })).data;
    path = setup.path ?? "";
    error = null;
  } catch (err) {
    error = text(err);
  } finally {
    busy = false;
  }
}

async function savePath(value: string | null): Promise<void> {
  if (busy) return;
  busy = true;
  pathError = null;
  try {
    setup = (await transport.call({ method: "agents.set_path", params: { agent: "claude", path: value } })).data;
    path = setup.path ?? "";
    editing = false;
  } catch (err) {
    pathError = text(err);
  } finally {
    busy = false;
  }
}
</script>

<div class="claude" data-testid="claude-status" aria-busy={setup === null && !error}>
  {#if ready && billing?.warning}
    <p class="warn" role="note" data-testid="claude-api-key">{billing.warning}</p>
  {/if}
  {#if setup === null && !error}
    <p class="muted">Checking {names.name()}…</p>
  {:else if setup && ready}
    <p class="ready"><strong>{advice(setup, names.name()).title}</strong></p>
    <dl>
      {#if billing?.paid}<dt>Billing</dt><dd data-testid="claude-billing">{billing.paid}</dd>{/if}
      <dt>Program</dt>
      <dd>
        <code data-testid="claude-executable">{setup.executable ?? "claude"}</code>
        <span class="muted">{setup.path ? "chosen by you" : "found automatically"}</span>
      </dd>
    </dl>
    {#if editing}
      <form
        onsubmit={(event) => {
          event.preventDefault();
          void savePath(path.trim() || null);
        }}
      >
        <label for="claude-pane-path">Path to claude</label>
        <input
          id="claude-pane-path"
          bind:value={path}
          spellcheck="false"
          autocomplete="off"
          placeholder="Empty: find it automatically"
          aria-invalid={pathError !== null}
          aria-describedby={pathError ? "claude-pane-path-error" : undefined}
        />
        {#if pathError}<p class="error" id="claude-pane-path-error" role="alert">{pathError}</p>{/if}
        <div class="row">
          <button type="submit" disabled={busy}>Save path</button>
          <button type="button" class="quiet" onclick={() => ((editing = false), (pathError = null))}>Cancel</button>
        </div>
      </form>
    {:else}
      <div class="row">
        <button type="button" class="quiet" disabled={busy} onclick={() => void check()}>Check again</button>
        <button type="button" class="quiet" onclick={() => (editing = true)}>Set path…</button>
      </div>
    {/if}
    <Accounts quiet />
  {:else}
    <!-- Something is wrong: the setup card says what, and how to fix it. -->
    {#if error}<p class="error" role="alert">Could not check {names.name()}: {error}</p>{/if}
    <SetupCard open />
  {/if}
</div>

<style>
  .claude {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: 480px;
  }
  p {
    margin: 0;
  }
  .ready strong {
    font-weight: 600;
  }
  .muted,
  label {
    font: var(--t-small);
    color: var(--text-2);
  }
  .warn {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: var(--sp-1) var(--sp-3);
    margin: 0;
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  code {
    font: 13px / 18px var(--mono);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .row {
    display: flex;
    gap: var(--sp-2);
    margin-top: var(--sp-1);
  }
  button,
  input {
    height: var(--ctl);
    padding: 0 var(--sp-3);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  button {
    cursor: default;
  }
  button.quiet {
    color: var(--text-2);
    background: transparent;
  }
  button.quiet:hover,
  button:hover {
    color: var(--text);
    background: var(--s4);
  }
  @media (forced-colors: active) {
    button,
    input {
      border: 1px solid ButtonText;
    }
  }
</style>
