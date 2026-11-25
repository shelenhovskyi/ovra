<script lang="ts">
// The Claude account a new session starts with, shown once there is more
// than one. Starts at the project's account, or at a signed-in one when the
// project's is signed out; a choice becomes the project's account, so the
// next session (and a task's workers) use it too. An account that bills an
// API key gets a warning under the picker; a signed-out one is the start
// form's to explain.
import { untrack } from "svelte";
import type { ProjectId } from "../../lib/protocol";
import { useOvra } from "../../shell/context";
import { pickerLabel, pickerWarning } from "./account-texts";
import { claudeAccounts } from "./claude-accounts.svelte";

let { project, account = $bindable(null) }: { project: ProjectId; account?: string | null } = $props();

const { transport } = useOvra();
const accounts = claudeAccounts(transport);
let failed = $state<string | null>(null);
/** The person picked one here: a reload of the accounts keeps it. */
let chosen = false;

$effect(() => {
  const id = project;
  chosen = false;
  untrack(() => void accounts.ensure(id).catch(() => {}));
});

$effect(() => {
  const many = accounts.list.length > 1;
  const preferred = accounts.preferred;
  untrack(() => {
    if (!many) account = null;
    else if (!chosen || account === null) account = preferred;
  });
});

const picked = $derived(accounts.list.find((s) => s.account.id === account));
const warning = $derived(picked ? pickerWarning(picked) : null);

function choose(id: string): void {
  account = id;
  chosen = true;
  failed = null;
  accounts.setProject(project, id).catch((err: unknown) => {
    failed = err instanceof Error ? err.message : String(err);
  });
}
</script>

{#if accounts.list.length > 1}
  <label class="picker" data-testid="account-picker" title="Remembered for this project">
    <span class="visually-hidden">Account</span>
    <select value={account} onchange={(event) => choose(event.currentTarget.value)}>
      {#each accounts.list as state (state.account.id)}
        <option value={state.account.id}>{pickerLabel(state)}</option>
      {/each}
    </select>
  </label>
  {#if warning}
    <p class="warning" role="note" data-testid="account-picker-warning">{warning}</p>
  {/if}
  {#if failed}
    <p class="error" role="alert">{failed}</p>
  {/if}
{/if}

<style>
  .picker {
    min-width: 0;
  }
  select {
    max-width: 100%;
    height: 32px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
    background: var(--s3);
    border: none;
    border-radius: var(--radius-sm);
  }
  .warning,
  .error {
    flex-basis: 100%;
    margin: 0;
    font-size: 13px;
  }
  .warning {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  @media (forced-colors: active) {
    select {
      border: 1px solid ButtonText;
    }
  }
</style>
