<script lang="ts">
// One agent other than Claude in Settings > Agents: whether it is installed
// and signed in, what to do if not, and the program Ovra runs.
import { providers } from "../agents/providers.svelte";
import { programOf, providerState } from "../agents/setup/provider-texts";
import type { AgentKind } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

let { id }: { id: AgentKind } = $props();

const { transport } = useOvra();
const names = providers(transport);
let editing = $state(false);
let path = $state("");
let pathError = $state<string | null>(null);
let busy = $state(false);

const setup = $derived(names.setups[id]);
const said = $derived(providerState(id, names.name(id), setup));

async function save(value: string | null): Promise<void> {
  if (busy) return;
  busy = true;
  pathError = null;
  try {
    await names.setPath(id, value);
    editing = false;
  } catch (err) {
    pathError = err instanceof CoreError ? err.error.message : err instanceof Error ? err.message : String(err);
  } finally {
    busy = false;
  }
}

async function again(): Promise<void> {
  busy = true;
  await names.check();
  busy = false;
}
</script>

<section class="card" data-testid={`provider-${id}`} aria-labelledby={`provider-${id}-name`}>
  <h4 id={`provider-${id}-name`}>{names.name(id)}</h4>
  <p>{said.line}</p>
  {#if said.fix}<p class="fix"><code>{said.fix}</code></p>{/if}
  {#if setup?.executable}
    <p class="muted"><code>{setup.executable}</code> {setup.path ? "chosen by you" : "found automatically"}</p>
  {/if}
  {#if editing}
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void save(path.trim() || null);
      }}
    >
      <label for={`provider-${id}-path`}>Path to {programOf(id)}</label>
      <input
        id={`provider-${id}-path`}
        bind:value={path}
        spellcheck="false"
        autocomplete="off"
        placeholder="Empty: find it automatically"
        aria-invalid={pathError !== null}
      />
      {#if pathError}<p class="error" role="alert">{pathError}</p>{/if}
      <div class="row">
        <button type="submit" disabled={busy}>Save path</button>
        <button type="button" class="quiet" onclick={() => ((editing = false), (pathError = null))}>Cancel</button>
      </div>
    </form>
  {:else}
    <div class="row">
      <button type="button" class="quiet" disabled={busy} onclick={() => void again()}>Check again</button>
      <button type="button" class="quiet" onclick={() => ((path = setup?.path ?? ""), (editing = true))}>Set path…</button>
    </div>
  {/if}
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 480px;
    padding-top: var(--sp-3);
    margin-top: var(--sp-3);
    border-top: 1px solid var(--line);
  }
  h4 {
    margin: 0;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .muted,
  label {
    font: var(--t-small);
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
  code {
    font: 13px / 18px var(--mono);
    overflow-wrap: anywhere;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .row {
    display: flex;
    gap: var(--sp-2);
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
