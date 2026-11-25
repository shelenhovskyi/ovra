<script lang="ts">
// The elements picked on a page and not sent yet, each with an optional note
// for the agent. The session's message box shows them as chips too.
import { tick } from "svelte";
import type { PageInfo } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { browserPicks, pickLabel } from "./picks.svelte";

/** `onEmptied` runs after the last pick is removed, to move focus on. */
let { page, onEmptied }: { page: PageInfo; onEmptied: () => void } = $props();

const picks = browserPicks(useOvra().transport);
const mine = $derived(picks.of(page.session).filter((p) => p.page === page.id));
const removeButtons: Record<string, HTMLButtonElement> = $state({});

/** Removes a pick; focus goes to the next one's Remove, else the previous one's. */
async function remove(pickId: string): Promise<void> {
  const at = mine.findIndex((p) => p.pick.pick_id === pickId);
  const neighbour = mine[at + 1] ?? mine[at - 1];
  picks.remove(page.session, pickId);
  await tick();
  if (neighbour) removeButtons[neighbour.pick.pick_id]?.focus();
  else onEmptied();
}
</script>

{#if mine.length > 0}
  <ol class="picks" aria-label="Picked on this page">
    {#each mine as p (p.pick.pick_id)}
      <li data-testid="browser-pick-item">
        <span class="name">{pickLabel(p.pick)}</span>
        <input
          type="text"
          placeholder="Note for the agent (optional)"
          aria-label={`Note for ${pickLabel(p.pick)}`}
          value={p.pick.note ?? ""}
          oninput={(event) => picks.note(page.session, p.pick.pick_id, event.currentTarget.value)}
        />
        <button
          type="button"
          aria-label={`Remove ${pickLabel(p.pick)}`}
          bind:this={removeButtons[p.pick.pick_id]}
          onclick={() => void remove(p.pick.pick_id)}>Remove</button
        >
      </li>
    {/each}
  </ol>
  <p class="hint">Picks go with your next message to this session.</p>
{/if}

<style>
  .picks {
    margin: 0;
    padding: 0 0 0 20px;
  }
  .picks li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .name {
    min-width: 0;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    color: inherit;
    padding: 4px 10px;
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
  }
  button {
    font: inherit;
    padding: 4px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--radius);
    cursor: pointer;
  }
</style>
