<script lang="ts">
// Settings > Agents: whether the task form offers several attempts at one
// task. Off by default, so the form stays as simple as it is.
import { agentPrefs } from "../agents/prefs.svelte";
</script>

<div class="attempts">
  <label class="check">
    <input
      type="checkbox"
      role="switch"
      bind:checked={agentPrefs.attempts}
      aria-describedby="agent-attempts-why"
      data-testid="offer-attempts"
    />
    Offer several attempts
  </label>
  <p class="hint" id="agent-attempts-why">
    With "One agent, own copy", up to three agents try the same task, each in its own copy. You compare them in Reviews,
    keep the best and discard the rest. Each attempt counts as an agent working.
  </p>
</div>

<style>
  .attempts {
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
