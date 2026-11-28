<script lang="ts">
// The warning that the project moved on since a worker started, above Keep,
// and the button that brings the project's changes into the worker's copy
// (`docs/specs/main-moved.md`).
import type { ReviewItem } from "../lib/protocol";
import { behindText, resolveComment, updatedText } from "./behind";
import type { ReviewInbox } from "./reviews.svelte";

let {
  inbox,
  item,
  onsaid,
  onask,
}: {
  inbox: ReviewInbox;
  item: ReviewItem;
  onsaid: (text: string) => void;
  /** Puts a comment for the agent in the comment box, for the person to send. */
  onask: (text: string) => void;
} = $props();

const busy = $derived(inbox.busy.includes(item.id));
const updated = $derived(inbox.updated?.id === item.id ? inbox.updated : null);
/** Conflicts found, nothing written yet: the person may write them with markers. */
const asking = $derived(updated !== null && !updated.written && updated.conflicts.length > 0);
const marked = $derived(updated?.written ? updated.conflicts : []);

async function update(conflicts: boolean): Promise<void> {
  if (busy) return;
  if (await inbox.update(item.id, conflicts)) {
    if (inbox.updated) onsaid(updatedText(inbox.updated));
  }
}
</script>

{#if item.behind || updated}
  <div class="behind" role="group" aria-label="Your project changed" data-testid="inbox-behind">
    {#if updated}
      <p class="hint" data-testid="inbox-updated">{updatedText(updated)}</p>
    {/if}
    {#if item.behind && !asking}
      <p class={item.behind.clashes.length > 0 ? "warn" : "hint"} data-testid="inbox-behind-text">
        {behindText(item.behind)}
      </p>
    {/if}
    <div class="buttons">
      {#if asking}
        <button type="button" data-testid="inbox-update-conflicts" aria-disabled={busy} onclick={() => update(true)}
          >Bring in and mark conflicts</button
        >
        <button type="button" class="quiet" onclick={() => (inbox.updated = null)}>Cancel</button>
      {:else if item.behind}
        <button
          type="button"
          class={item.behind.clashes.length > 0 ? "" : "quiet"}
          data-testid="inbox-update"
          aria-disabled={busy}
          onclick={() => update(false)}>Bring in project changes</button
        >
      {/if}
      {#if marked.length > 0}
        <button type="button" data-testid="inbox-ask-resolve" onclick={() => onask(resolveComment(marked))}
          >Ask the agent to resolve them</button
        >
      {/if}
    </div>
  </div>
{/if}

<style>
  .behind {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  p {
    margin: 0;
    font-size: var(--fs-ui);
  }
  .hint {
    color: var(--text-2);
  }
  .warn {
    color: var(--text);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .buttons:empty {
    display: none;
  }
  button {
    min-height: var(--control-h-sm);
    padding: 5px 12px;
    font: inherit;
    font-weight: 500;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-control);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  button.quiet {
    color: var(--text-2);
    background: transparent;
  }
  button.quiet:hover {
    background: var(--s3);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
    cursor: default;
  }
  @media (forced-colors: active) {
    button[aria-disabled="true"] {
      color: GrayText;
    }
  }
</style>
