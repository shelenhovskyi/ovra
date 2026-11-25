<script lang="ts">
// Claude accounts (personal, work, ...): each a Claude Code configuration
// folder with its own sign-in. Lists them with their sign-in state, offers
// the folders found on this computer and adds, renames and removes them.
import { untrack } from "svelte";
import type { AccountState } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { useOvra } from "../../shell/context";
import { accountStatus, accountsSummary, nameFromFolder } from "./account-texts";
import { claudeAccounts, DEFAULT_ACCOUNT } from "./claude-accounts.svelte";

/** `quiet`: the setup card is one line; with one account and none found,
 * nothing shows until the person opens the card. */
let { quiet = false }: { quiet?: boolean } = $props();

const { transport, workspace } = useOvra();
const accounts = claudeAccounts(transport);

let error = $state<string | null>(null);
let busy = $state(false);
let adding = $state(false);
let managing = $state(false);
let name = $state("");
let folder = $state("");
/** The account being renamed, and its new name. */
let renaming = $state<string | null>(null);
let newName = $state("");

const list = $derived(accounts.list);
const found = $derived(accounts.overview?.found ?? []);
/** Folded to one line until the person manages them or adds one. */
const open = $derived(managing || adding);

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => void run(() => accounts.ensure(project)));
});

function text(err: unknown): string {
  if (err instanceof CoreError) return err.error.message;
  return err instanceof Error ? err.message : String(err);
}

async function run(action: () => Promise<void>): Promise<boolean> {
  if (busy) return false;
  busy = true;
  error = null;
  try {
    await action();
    return true;
  } catch (err) {
    error = text(err);
    return false;
  } finally {
    busy = false;
  }
}

async function add(accountName: string, configDir: string): Promise<void> {
  const saved = await run(() => accounts.save({ id: "", name: accountName, config_dir: configDir }));
  if (saved) {
    adding = false;
    name = "";
    folder = "";
  }
}

async function rename(state: AccountState): Promise<void> {
  if (await run(() => accounts.save({ ...state.account, name: newName }))) renaming = null;
}

function startRename(state: AccountState): void {
  renaming = state.account.id;
  newName = state.account.name;
}
</script>

{#if !quiet || list.length > 1 || found.length > 0 || open}
<div class="accounts" data-testid="claude-accounts">
  {#if !open}
    {#if list.length > 1}
      <div class="row">
        <span class="detail" data-testid="claude-accounts-summary">Accounts: {accountsSummary(list)}</span>
        <button type="button" class="link" onclick={() => (managing = true)}>Manage</button>
      </div>
    {:else}
      <button type="button" class="link" onclick={() => (adding = true)}
        >Use another Claude account (work, personal)</button
      >
    {/if}
    {#each found as dir (dir)}
      <p class="found" data-testid="claude-account-found">
        <span class="detail">Found another account's folder: {dir}</span>
        <button type="button" disabled={busy} onclick={() => void add(nameFromFolder(dir), dir)}
          >Add as {nameFromFolder(dir)}</button
        >
      </p>
    {/each}
    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  {:else}
  <div class="row">
    <h4 id="claude-accounts-heading">Claude accounts</h4>
    <span class="buttons">
      <button type="button" disabled={busy} onclick={() => void run(() => accounts.load())}>Check again</button>
      <button type="button" onclick={() => ((managing = false), (adding = false))}>Done</button>
    </span>
  </div>
  <ul aria-labelledby="claude-accounts-heading">
    {#each list as state (state.account.id)}
      {@const status = accountStatus(state)}
      <li data-testid="claude-account">
        {#if renaming === state.account.id}
          <form
            class="inline"
            onsubmit={(event) => {
              event.preventDefault();
              void rename(state);
            }}
          >
            <label class="hidden" for="account-rename">New name</label>
            <input id="account-rename" bind:value={newName} autocomplete="off" />
            <button type="submit" disabled={busy || !newName.trim()}>Save</button>
            <button type="button" onclick={() => (renaming = null)}>Cancel</button>
          </form>
        {:else}
          <div class="row">
            <strong>{state.account.name}</strong>
            <span class="buttons">
              <button type="button" class="link" onclick={() => startRename(state)}>Rename</button>
              {#if state.account.id !== DEFAULT_ACCOUNT}
                <button
                  type="button"
                  class="link"
                  disabled={busy}
                  aria-label={`Remove ${state.account.name}`}
                  onclick={() => void run(() => accounts.remove(state.account.id))}>Remove</button
                >
              {/if}
            </span>
          </div>
        {/if}
        <span class="detail">{state.account.config_dir ?? "Claude Code's own folder (~/.claude)"}</span>
        <span class:problem={!status.ok} class="detail" data-testid="claude-account-status">{status.text}</span>
        {#if status.command}
          <code>{status.command}</code>
        {/if}
      </li>
    {/each}
  </ul>
  {#each found as dir (dir)}
    <p class="found" data-testid="claude-account-found">
      <span class="detail">Found {dir}</span>
      <button type="button" disabled={busy} onclick={() => void add(nameFromFolder(dir), dir)}
        >Add as {nameFromFolder(dir)}</button
      >
    </p>
  {/each}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  {#if adding}
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void add(name, folder);
      }}
    >
      <label for="account-name">Name</label>
      <input id="account-name" bind:value={name} autocomplete="off" placeholder="Work" />
      <label for="account-folder">Configuration folder</label>
      <input
        id="account-folder"
        bind:value={folder}
        spellcheck="false"
        autocomplete="off"
        placeholder="/Users/you/.claude-work"
        aria-describedby="account-folder-hint"
      />
      <span class="detail" id="account-folder-hint"
        >The folder you give Claude Code as CLAUDE_CONFIG_DIR for this account. Use the full path.</span
      >
      <div class="row end">
        <button type="button" onclick={() => (adding = false)}>Cancel</button>
        <button type="submit" disabled={busy || !name.trim() || !folder.trim()}>Add account</button>
      </div>
    </form>
  {:else}
    <button type="button" class="link" onclick={() => (adding = true)}>Add an account</button>
  {/if}
  {/if}
</div>
{/if}

<style>
  .accounts {
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-top: 1px solid #2a2a31;
    padding-top: 4px;
  }
  h4 {
    margin: 0;
    font-size: 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .row.end {
    justify-content: flex-end;
  }
  .buttons {
    display: flex;
    gap: 8px;
  }
  p {
    margin: 0;
  }
  .found {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .detail,
  label {
    color: #b4b4c0;
    overflow-wrap: anywhere;
  }
  .detail.problem {
    color: #ffd36b;
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
  form.inline {
    flex-direction: row;
    align-items: center;
  }
  form.inline input {
    flex: 1;
    min-width: 0;
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
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
</style>
