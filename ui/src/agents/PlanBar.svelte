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

<div class="plan" aria-live="polite" data-testid="plan-bar">
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
  /* A component of its own: the agents panel's button styles do not reach it. */
  .primary {
    flex: none;
    height: var(--ctl-lg);
    padding: 0 14px;
    font: inherit;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: 1px solid transparent;
    border-radius: var(--r-control);
  }
  .primary:disabled {
    color: var(--text-2);
    background: var(--s3);
  }
  p {
    margin: 0;
    font: var(--t-small);
    color: var(--text-2);
  }
</style>
