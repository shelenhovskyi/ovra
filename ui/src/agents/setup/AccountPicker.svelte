<script lang="ts">
// The Claude account a new session starts with, shown once there is more
// than one. Starts at the project's account; a choice becomes the project's
// account, so the next session (and a task's workers) use it too.
import { untrack } from "svelte";
import type { ProjectId } from "../../lib/protocol";
import { useOvra } from "../../shell/context";
import { claudeAccounts } from "./accounts.svelte";

let { project, account = $bindable(null) }: { project: ProjectId; account?: string | null } = $props();

const { transport } = useOvra();
const accounts = claudeAccounts(transport);
let failed = $state<string | null>(null);

$effect(() => {
  const id = project;
  untrack(() => void accounts.ensure(id).catch(() => {}));
});

$effect(() => {
  account = accounts.list.length > 1 ? accounts.projectAccount : null;
});

function choose(id: string): void {
  account = id;
  failed = null;
  accounts.setProject(project, id).catch((err: unknown) => {
    failed = err instanceof Error ? err.message : String(err);
  });
}
</script>

{#if accounts.list.length > 1}
  <label class="picker" data-testid="account-picker">
    <span>Account</span>
    <select value={account} onchange={(event) => choose(event.currentTarget.value)}>
      {#each accounts.list as state (state.account.id)}
        <option value={state.account.id}>{state.account.name}</option>
      {/each}
    </select>
  </label>
  {#if failed}
    <span class="error" role="alert">{failed}</span>
  {/if}
{/if}

<style>
  .picker {
    display: flex;
    align-items: center;
    gap: 4px;
    color: #b4b4c0;
    font-size: 12px;
  }
  select {
    font: inherit;
    color: #e6e6e6;
    background: #121216;
    border: 1px solid #4a4a57;
    border-radius: 3px;
    padding: 1px 2px;
  }
  select:focus-visible {
    outline: 2px solid #7aa2ff;
    outline-offset: 1px;
  }
  .error {
    color: #ff8f8f;
    font-size: 12px;
  }
</style>
