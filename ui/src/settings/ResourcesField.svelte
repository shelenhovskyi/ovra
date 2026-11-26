<script lang="ts">
// Settings > Agents: whether the agents list shows each agent's memory and
// CPU. Off by default; why a worker waits shows either way.
import { agentPrefs } from "../agents/prefs.svelte";
</script>

<div class="resources">
  <label class="check">
    <input
      type="checkbox"
      role="switch"
      bind:checked={agentPrefs.showResources}
      aria-describedby="agent-resources-why"
      data-testid="show-resources"
    />
    Show resource usage
  </label>
  <p class="hint" id="agent-resources-why">
    Each agent's memory and CPU in the agents list. Ovra holds new workers back while memory is low or the CPU is busy
    either way, and says so.
  </p>
</div>

<style>
  .resources {
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
