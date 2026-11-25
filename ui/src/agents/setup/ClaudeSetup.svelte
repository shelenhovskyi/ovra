<script lang="ts">
// Onboarding: is the user's Claude installed, supported and signed in? Says
// how to fix it when not, and lets the user choose the executable's path.
// Once Claude is ready the card is one line; "Change" opens the rest.
import { untrack } from "svelte";
import type { AgentSetup } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import Accounts from "./Accounts.svelte";
import { advice, billingText, INSTALL_COMMANDS, paidShort, SETUP_DOCS } from "./texts";

const { transport } = useOvra();

let setup = $state<AgentSetup | null>(null);
let error = $state<string | null>(null);
// A path the core refused (no file there, say), shown under the field.
let pathError = $state<string | null>(null);
let busy = $state(false);
let editing = $state(false);
let path = $state("");

const tip = $derived(setup ? advice(setup) : null);
const ready = $derived(setup?.state.state === "ready");
const compact = $derived(ready && !editing);
const billing = $derived(setup && ready ? billingText(setup) : null);
const paid = $derived(setup && ready ? paidShort(setup) : null);

$effect(() => {
  untrack(() => void check());
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

<section
  class="setup"
  class:problem={setup && !ready}
  aria-labelledby="claude-setup-heading"
  data-testid="claude-setup"
>
  {#if compact}
    <div class="row">
      <div class="line">
        <h3 id="claude-setup-heading">{tip?.title}</h3>
        {#if paid}<span class="detail" data-testid="claude-billing"> · {paid}</span>{/if}
      </div>
      <button type="button" class="link" onclick={() => (editing = true)}>Change</button>
    </div>
  {:else}
    <div class="row">
      <h3 id="claude-setup-heading">{tip?.title ?? "Checking Claude…"}</h3>
      <button type="button" disabled={busy} onclick={check}>Check again</button>
    </div>
  {/if}
  {#if tip && !compact}
    {#if tip.detail}
      <p class="detail">{tip.detail}</p>
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
  <Accounts quiet={compact} />
</section>

<style>
  .setup {
    margin: 0 12px 8px;
    padding: 6px 8px;
    border: 1px solid #2a2a31;
    border-radius: 3px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
  }
  .setup.problem {
    border-color: #ffd36b;
  }
  h3 {
    margin: 0;
    font-size: 12px;
  }
  .line h3 {
    display: inline;
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
  .install span {
    color: #b4b4c0;
    overflow-wrap: anywhere;
  }
  .install {
    list-style: none;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .install li {
    display: flex;
    flex-direction: column;
  }
  code {
    user-select: all;
    overflow-wrap: anywhere;
    font-family: ui-monospace, monospace;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  input {
    font: inherit;
    color: inherit;
    background: #121216;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    padding: 2px 4px;
  }
  input[aria-invalid="true"] {
    border-color: #ff8f8f;
  }
  button {
    font: inherit;
    padding: 2px 8px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
  }
  button.primary {
    border-color: #7aa2ff;
  }
  button.link {
    align-self: flex-start;
    padding: 0;
    background: none;
    border: none;
    color: #9db8ff;
    text-decoration: underline;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .error {
    color: #ff8f8f;
  }
  .warning {
    /* Who pays is what this card must get right (ADR 0006): it stands out. */
    font-weight: 600;
    color: #ffd36b;
  }
  @media (forced-colors: active) {
    .setup.problem {
      border: 2px solid Highlight;
    }
  }
</style>
