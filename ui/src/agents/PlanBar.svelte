<script lang="ts">
// Plan first (`docs/specs/plan-first.md`): while the agent plans it changes
// no file; once its turn ends the person approves the plan here, or replies
// to change it.
let {
  working,
  disabled = false,
  onapprove,
}: {
  /** A turn runs: the plan is not ready yet. */
  working: boolean;
  disabled?: boolean;
  onapprove: () => void;
} = $props();
</script>

<div class="plan" role="status" data-testid="plan-bar">
  {#if working}
    <p>Planning. The agent changes no files until you approve its plan.</p>
  {:else}
    <p>Plan ready. Approve it to let the agent change files, or reply to change the plan.</p>
    <button type="button" class="primary" {disabled} onclick={onapprove}>Approve plan</button>
  {/if}
</div>

<style>
  .plan {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  p {
    margin: 0;
    font: var(--t-small);
    color: var(--text-2);
  }
</style>
