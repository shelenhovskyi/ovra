<script lang="ts">
// Settings > Agents: whether the task form offers "Plan first". Off by
// default, so the form stays as simple as it is.
import { agentPrefs } from "../agents/prefs.svelte";
</script>

<div class="plan-first">
  <label class="check">
    <input
      type="checkbox"
      role="switch"
      bind:checked={agentPrefs.planFirst}
      aria-describedby="agent-plan-first-why"
      data-testid="offer-plan-first"
    />
    Offer "Plan first"
  </label>
  <p class="hint" id="agent-plan-first-why">
    The task form gets a Plan first box. The agent then looks around, shows you a plan and changes no file until you
    approve it; until then Ovra asks you before each command it runs.
  </p>
</div>

<style>
  .plan-first {
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
</style>
