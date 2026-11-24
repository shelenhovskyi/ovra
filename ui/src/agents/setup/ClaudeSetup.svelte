<script lang="ts">
// Onboarding: is the user's Claude installed, supported and signed in? Says
// how to fix it when not, and lets the user choose the executable's path.
import { untrack } from "svelte";
import type { AgentSetup } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import { advice, INSTALL_COMMANDS, SETUP_DOCS } from "./texts";

const { transport } = useOvra();

let setup = $state<AgentSetup | null>(null);
let error = $state<string | null>(null);
let busy = $state(false);
let editing = $state(false);
let path = $state("");

const tip = $derived(setup ? advice(setup) : null);
const ready = $derived(setup?.state.state === "ready");

$effect(() => {
  untrack(() => void check());
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function run(action: () => Promise<AgentSetup>): Promise<void> {
  if (busy) return;
  busy = true;
  error = null;
  try {
    setup = await action();
    path = setup.path ?? "";
  } catch (err) {
    error = text(err);
  } finally {
    busy = false;
  }
}

function check(): Promise<void> {
  return run(async () => (await transport.call({ method: "agents.setup", params: { agent: "claude" } })).data);
}

function savePath(value: string | null): Promise<void> {
  return run(async () => {
    const saved = await transport.call({ method: "agents.set_path", params: { agent: "claude", path: value } });
    if (saved.data.state.state === "ready") editing = false;
    return saved.data;
  });
}

function openDocs(): void {
  transport.call({ method: "open.url", params: { url: SETUP_DOCS } }).catch((err: unknown) => (error = text(err)));
}
</script>

<section class="setup" class:problem={setup && !ready} aria-labelledby="claude-setup-heading" data-testid="claude-setup">
  <div class="row">
    <h3 id="claude-setup-heading">{tip?.title ?? "Checking Claude…"}</h3>
    <button type="button" disabled={busy} onclick={check}>Check again</button>
  </div>
  {#if tip}
    <p class="detail">{tip.detail}</p>
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
  {/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  {#if editing || (setup && !ready)}
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
        placeholder="Empty: search PATH and the usual install folders"
      />
      <div class="row">
        {#if setup?.path}
          <button type="button" disabled={busy} onclick={() => savePath(null)}>Search automatically</button>
        {/if}
        <button type="submit" disabled={busy}>Save path</button>
      </div>
    </form>
  {:else if setup}
    <button type="button" class="link" onclick={() => (editing = true)}>Change path</button>
  {/if}
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
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  form .row {
    justify-content: flex-end;
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
  button {
    font: inherit;
    padding: 2px 8px;
    color: #e6e6e6;
    background: #2a2a33;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    cursor: pointer;
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
  @media (forced-colors: active) {
    .setup.problem {
      border: 2px solid Highlight;
    }
  }
</style>
