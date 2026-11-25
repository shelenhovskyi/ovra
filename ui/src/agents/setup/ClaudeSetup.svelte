<script lang="ts">
// Onboarding: is the user's Claude installed, supported and signed in? Says
// how to fix it when not, and lets the user choose the executable's path.
// Once the account new tasks start with is ready the card goes (an API key
// in the environment still warns); `open` shows it again, from the start
// form's Claude button.
import { tick, untrack } from "svelte";
import type { AgentSetup } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import Accounts from "./Accounts.svelte";
import { setupAsks } from "./asks.svelte";
import { claudeAccounts } from "./claude-accounts.svelte";
import { advice, billingText, INSTALL_COMMANDS, paidShort, SETUP_DOCS } from "./texts";

let { open = false }: { open?: boolean } = $props();

const { transport } = useOvra();
const accounts = claudeAccounts(transport);

let setup = $state<AgentSetup | null>(null);
let error = $state<string | null>(null);
// A path the core refused (no file there, say), shown under the field.
let pathError = $state<string | null>(null);
let busy = $state(false);
/** The first check has not answered yet: a quiet line holds the card's place. */
let first = $state(true);
let editing = $state(false);
let path = $state("");

const tip = $derived(setup ? advice(setup) : null);
const ready = $derived(setup?.state.state === "ready");
const compact = $derived(ready && !editing);
const billing = $derived(setup && ready ? billingText(setup) : null);
const paid = $derived(setup && ready ? paidShort(setup) : null);
/** The account new tasks start with can run them; with several, the picker's, not the default one. */
const usable = $derived.by(() => {
  const chosen = accounts.list.length > 1 ? accounts.state(accounts.preferred) : undefined;
  return chosen ? chosen.setup.state.state === "ready" : ready;
});
/** Nothing to do: no card, only an API key warning. Also while the first check runs. */
const hidden = $derived(!open && !editing && (first || usable));

$effect(() => {
  untrack(() => void check());
});

// "Set path to claude…" from the palette: the path field, focused.
$effect(() => {
  if (!setupAsks.path) return;
  untrack(() => {
    setupAsks.path = false;
    editing = true;
    void tick().then(() => document.getElementById("claude-path")?.focus());
  });
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function run(
  action: () => Promise<AgentSetup>,
  onError = (message: string): void => {
    error = message;
  },
): Promise<void> {
  if (busy) return;
  busy = true;
  error = null;
  pathError = null;
  try {
    setup = await action();
    path = setup.path ?? "";
  } catch (err) {
    onError(text(err));
  } finally {
    busy = false;
    first = false;
  }
}

function check(): Promise<void> {
  return run(async () => (await transport.call({ method: "agents.setup", params: { agent: "claude" } })).data);
}

function savePath(value: string | null): Promise<void> {
  return run(
    async () => {
      const saved = await transport.call({ method: "agents.set_path", params: { agent: "claude", path: value } });
      if (saved.data.state.state === "ready") editing = false;
      return saved.data;
    },
    (message) => {
      if (value === null) error = message;
      else pathError = message;
    },
  );
}

function openDocs(): void {
  transport.call({ method: "open.url", params: { url: SETUP_DOCS } }).catch((err: unknown) => (error = text(err)));
}
</script>

{#if hidden}
  {#if billing?.warning}
    <p class="warning alone" role="note" data-testid="claude-api-key">{billing.warning}</p>
  {/if}
{:else}
<section
  class="setup"
  class:problem={setup && !ready}
  aria-labelledby="claude-setup-heading"
  aria-busy={first}
  data-testid="claude-setup"
>
  {#if first}
    <!-- The check usually answers within a moment: one quiet line, as tall
         as the ready card's, instead of a status and links that flash. -->
    <div class="row">
      <h3 id="claude-setup-heading" class="pending">Claude</h3>
    </div>
  {:else if compact}
    <div class="row">
      <div class="line">
        <h3 id="claude-setup-heading">{tip?.title}</h3>
        {#if paid}<span class="detail" data-testid="claude-billing"> · {paid}</span>{/if}
      </div>
      <button type="button" class="link" onclick={() => (editing = true)}>Change</button>
    </div>
  {:else}
    <div class="row">
      <h3 id="claude-setup-heading">{tip?.title ?? "Could not check Claude"}</h3>
      <button type="button" class:primary={!tip?.findAutomatically} disabled={busy} onclick={check}>Check again</button>
    </div>
  {/if}
  {#if tip && !compact}
    {#if tip.detail}
      <details class="how">
        <summary>How to fix</summary>
        <p class="detail">{tip.detail}</p>
      </details>
    {/if}
    {#if tip.command}
      <p class="command"><code>{tip.command}</code></p>
    {/if}
    {#if tip.install}
      <ul class="install" aria-label="Install commands">
        {#each INSTALL_COMMANDS as option (option.label)}
          <li><span>{option.label}</span><code>{option.command}</code></li>
        {/each}
      </ul>
      <button type="button" class="link" onclick={openDocs}>Installation guide</button>
    {/if}
    {#if setup?.executable && setup.state.state !== "not_found"}
      <p class="detail" data-testid="claude-executable">Using {setup.executable}</p>
    {/if}
    {#if billing?.paid}
      <p class="detail" data-testid="claude-billing">{billing.paid}</p>
    {/if}
  {/if}
  {#if billing?.warning}
    <p class="warning" role="note" data-testid="claude-api-key">{billing.warning}</p>
  {/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  {#if tip?.findAutomatically && !editing}
    <div class="row start">
      <button type="button" class="primary" disabled={busy} onclick={() => savePath(null)}>Find automatically</button>
      <button type="button" class="link" onclick={() => (editing = true)}>Choose another path</button>
    </div>
  {:else if editing}
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void savePath(path.trim() || null);
      }}
    >
      <label for="claude-path">Path to claude</label>
      <input
        id="claude-path"
        bind:value={path}
        spellcheck="false"
        autocomplete="off"
        aria-invalid={pathError !== null}
        aria-describedby={pathError ? "claude-path-error" : undefined}
        placeholder="Empty: search PATH and the usual install folders"
      />
      {#if pathError}
        <p class="error" id="claude-path-error" role="alert" data-testid="claude-path-error">{pathError}</p>
      {/if}
      <div class="row">
        <button type="button" class="link" onclick={() => ((editing = false), (pathError = null))}>Cancel</button>
        <span class="actions">
          {#if setup?.path}
            <button type="button" disabled={busy} onclick={() => savePath(null)}>Find automatically</button>
          {/if}
          <button type="submit" disabled={busy}>Save path</button>
        </span>
      </div>
    </form>
  {:else if setup && !ready}
    <button type="button" class="link" onclick={() => (editing = true)}>Choose a path</button>
  {/if}
  {#if !first}<Accounts quiet={compact} />{/if}
</section>
{/if}

<style>
  .setup {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  /* Ready: one quiet line, no card. */
  .setup:not(.problem) {
    padding: 4px 8px;
    background: none;
  }
  h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .problem h3 {
    color: var(--warn);
  }
  .line h3 {
    display: inline;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
  }
  h3.pending {
    font-weight: 400;
    color: var(--text-2);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .row.start {
    justify-content: flex-start;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  p,
  ul {
    margin: 0;
  }
  .detail,
  label,
  .install span,
  summary {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  summary {
    width: fit-content;
    cursor: pointer;
  }
  .install {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .install li {
    display: flex;
    flex-direction: column;
  }
  code {
    padding: 4px 8px;
    background: var(--s3);
    border-radius: var(--radius-sm);
    user-select: all;
    overflow-wrap: anywhere;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  input {
    height: 32px;
    padding: 0 8px;
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: none;
    border-radius: var(--radius-sm);
  }
  input[aria-invalid="true"] {
    box-shadow: inset 0 0 0 1px var(--danger);
  }
  button {
    height: 28px;
    padding: 0 10px;
    font: inherit;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:not(.primary):hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  button.primary {
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  button.link {
    align-self: flex-start;
    padding: 0 4px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    color: var(--danger);
  }
  .warning.alone {
    margin: 0;
    padding: 4px 8px;
    font-size: 13px;
  }
  .warning {
    /* Who pays is what this card must get right (ADR 0006): it stands out. */
    font-weight: 600;
    color: var(--warn);
  }
  @media (forced-colors: active) {
    .setup.problem {
      border: 2px solid Highlight;
    }
  }
</style>

