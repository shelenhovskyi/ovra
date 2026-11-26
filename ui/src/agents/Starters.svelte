<script lang="ts">
// The empty agents screen's ready-made tasks: one press starts one.
import { STARTERS, type Starter } from "./starters";

let { disabled, pick }: { disabled: boolean; pick: (starter: Starter) => void } = $props();
</script>

<section class="starters" aria-labelledby="starters-heading" data-testid="agent-starters">
  <h3 id="starters-heading">Start with one of these</h3>
  <ul>
    {#each STARTERS as starter (starter.id)}
      <li>
        <button type="button" data-starter={starter.id} {disabled} onclick={() => pick(starter)}>
          <span class="label">{starter.label}</span>
          <span class="what">{starter.what}</span>
        </button>
      </li>
    {/each}
  </ul>
</section>

<style>
  .starters {
    margin: 0 8px 8px;
  }
  h3 {
    margin: 0 4px 6px;
    font: var(--t-caption);
    color: var(--text-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  button {
    width: 100%;
    height: auto;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    text-align: left;
    background: var(--s2);
    border: 0;
    border-radius: var(--radius);
    color: var(--text);
  }
  button:hover:not(:disabled) {
    background: var(--s3);
  }
  .label {
    font: var(--t-body);
    font-weight: 500;
  }
  .what {
    font: var(--t-small);
    color: var(--text-2);
  }
</style>
