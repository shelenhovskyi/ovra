<script lang="ts">
// The messages the person queued while the agent works
// (`docs/specs/plan-first.md`): each goes when a turn ends. Edit takes one
// back into the reply box; Send now stops the turn, so the first one goes.
import type { QueuedMessage } from "../lib/protocol/agents";

let {
  queue,
  working,
  disabled = false,
  onedit,
  onremove,
  onsendnow,
}: {
  queue: QueuedMessage[];
  /** A turn runs: Send now can stop it. */
  working: boolean;
  disabled?: boolean;
  onedit: (message: QueuedMessage) => void;
  onremove: (message: QueuedMessage) => void;
  onsendnow: () => void;
} = $props();
</script>

{#if queue.length > 0}
  <section class="queued" aria-label="Queued messages" data-testid="queued-messages">
    <div class="head">
      <span>{queue.length === 1 ? "Sends" : `${queue.length} messages send one by one`} when the agent finishes this step</span>
      {#if working}
        <button type="button" class="quiet soft" {disabled} title="Stops the agent's current step" onclick={onsendnow}
          >Send now</button
        >
      {/if}
    </div>
    <ol>
      {#each queue as message (message.id)}
        <li data-testid="queued-message">
          <span class="text" title={message.text}>{message.text}</span>
          <button type="button" class="quiet soft" {disabled} aria-label={`Edit "${message.text}"`} onclick={() => onedit(message)}
            >Edit</button
          >
          <button type="button" class="quiet soft" {disabled} aria-label={`Remove "${message.text}"`} onclick={() => onremove(message)}
            >Remove</button
          >
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  .queued {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    font: var(--t-small);
    color: var(--text-2);
  }
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  /* A component of its own: the agents panel's button styles do not reach it. */
  .quiet {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: 28px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius-sm);
  }
  .quiet:hover:not(:disabled) {
    background: var(--s4);
  }
  .quiet:disabled {
    opacity: 0.5;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
