<script lang="ts">
// Settings > Agents: whether schedules start agents by themselves (ADR
// 0015). Off by default; on, each project's Settings get a Schedules pane.
import { CoreError } from "../lib/transport";
import { openSettings } from "../settings/commands";
import { useOvra } from "../shell/context";
import { schedules } from "./schedules.svelte";

const { transport, workspace } = useOvra();
const store = schedules(transport);
let error = $state<string | null>(null);

async function toggle(input: HTMLInputElement): Promise<void> {
  error = null;
  try {
    await store.setOn(input.checked);
  } catch (err) {
    error = err instanceof CoreError ? err.error.message : String(err);
    input.checked = store.on === true;
  }
}
</script>

{#if store.on !== null}
  <div class="schedules-switch">
    <label class="check">
      <input
        type="checkbox"
        role="switch"
        checked={store.on}
        aria-describedby="schedules-switch-why"
        data-testid="schedules-switch"
        onchange={(e) => void toggle(e.currentTarget)}
      />
      Scheduled tasks
    </label>
    <p class="hint" id="schedules-switch-why">
      Agents start by themselves at the times you set, only while Ovra is open. Each run uses your plan like a task you
      start.
      {#if store.on && workspace.project}
        <button type="button" class="link" onclick={() => openSettings("schedules")}>Set up schedules</button>
      {/if}
    </p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .schedules-switch {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 480px;
    margin-top: var(--sp-3);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .hint {
    margin: 0;
    font: var(--t-small);
    color: var(--text-2);
  }
  .link {
    padding: 0;
    font: inherit;
    color: var(--accent);
    background: none;
    border: 0;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
</style>
