<script lang="ts">
// Settings > Agents: whether the task form offers other repositories to
// change in the same task. Off by default, so the form stays as simple as it is.
import { agentPrefs } from "../agents/prefs.svelte";
</script>

<div class="multi-repo">
  <label class="check">
    <input
      type="checkbox"
      role="switch"
      bind:checked={agentPrefs.multiRepo}
      aria-describedby="agent-multi-repo-why"
      data-testid="offer-multi-repo"
    />
    Tasks across several repositories
  </label>
  <p class="hint" id="agent-multi-repo-why">
    With "One agent, own copy", one agent can also change up to three of your other projects, such as a frontend and
    its backend. It gets its own copy of each, and Reviews shows one item per repository, together.
  </p>
</div>

<style>
  .multi-repo {
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
