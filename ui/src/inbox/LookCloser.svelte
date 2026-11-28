<script lang="ts">
// "Look closer": the lines of an item's diff where the agent changed tests
// or silenced checks, read by the core from the diff itself. Quiet, under
// the check line, and absent when there is nothing; a press opens that
// file's diff. Never blocks Keep: a test changed on purpose is normal work.
import type { ReviewItem, ReviewWarning } from "../lib/protocol";
import { closerIntro, closerRows } from "./look-closer";
import type { ReviewInbox } from "./reviews.svelte";

let { inbox, item, onopen }: { inbox: ReviewInbox; item: ReviewItem; onopen: (path: string) => void } = $props();

let shown = $state<{ id: string; warnings: ReviewWarning[] } | null>(null);
$effect(() => {
  // Read again whenever the core says an item changed.
  const id = item.id;
  void inbox.changes;
  void item.files;
  let live = true;
  inbox
    .warnings(id)
    .then((warnings) => {
      if (live) shown = { id, warnings };
    })
    .catch(() => {
      // Nothing to point at is better than a wrong pointer.
      if (live) shown = null;
    });
  return () => {
    live = false;
  };
});

const warnings = $derived(shown !== null && shown.id === item.id ? shown.warnings : []);
const rows = $derived(closerRows(warnings));
</script>

{#if rows.length > 0}
  <section class="closer" aria-labelledby="inbox-closer-title" data-testid="inbox-closer">
    <h4 id="inbox-closer-title">Look closer</h4>
    <p class="intro">{closerIntro(warnings)}</p>
    <ul>
      {#each rows as row (row.key)}
        <li>
          <button type="button" class="quiet" onclick={() => onopen(row.path)} title={row.path}>
            <span class="label">{row.label}</span>
            <span class="place">{row.place}</span>
          </button>
          {#if row.text}
            <code>{row.text}</code>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .closer {
    margin: 8px 0 0;
    font-size: var(--fs-ui);
    color: var(--text-2);
  }
  h4 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
    color: var(--text);
  }
  .intro {
    margin: 2px 0 4px;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 2px;
  }
  li {
    min-width: 0;
  }
  button {
    display: inline-flex;
    gap: 6px;
    align-items: baseline;
    max-width: 100%;
    padding: 0 4px;
    margin-inline-start: -4px;
    border: 0;
    border-radius: var(--r-control);
    color: var(--text-2);
    background: transparent;
    text-align: start;
  }
  button:hover {
    background: var(--s3);
  }
  .label {
    color: var(--text);
  }
  .place {
    overflow-wrap: anywhere;
  }
  code {
    display: block;
    margin: 0 0 2px 12px;
    font-family: var(--mono);
    font-size: var(--fs-ui-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--text-2);
  }
</style>
