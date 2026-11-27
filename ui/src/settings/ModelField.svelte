<script lang="ts">
// Settings > Agents: whether the task form offers a model for Claude. Off by
// default, so the form stays as simple as it is.
import { agentPrefs } from "../agents/prefs.svelte";
</script>

<div class="model">
  <label class="check">
    <input
      type="checkbox"
      role="switch"
      bind:checked={agentPrefs.pickModel}
      aria-describedby="agent-model-why"
      data-testid="offer-model"
    />
    Choose the model for each task
  </label>
  <p class="hint" id="agent-model-why">
    Adds a picker for Claude's tasks: Opus for hard work, Sonnet for most, Haiku for quick questions. "Default model"
    keeps your account's own choice.
  </p>
</div>

<style>
  .model {
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
