<script lang="ts" module>
/** A button of an empty state: a verb, the shortcut that does the same, what it runs. */
export type EmptyAction = { label: string; keys?: string | null; run: () => void };
</script>

<script lang="ts">
// What a panel shows when it has nothing to show (docs/design/patterns.md,
// "Empty states"): a headline, one line, and the next step as a quiet
// button with its key. Centred in the main area, top-left in side panels.
let {
  headline,
  line,
  actions = [],
  centred = false,
  testid,
}: {
  headline: string;
  line?: string;
  actions?: EmptyAction[];
  centred?: boolean;
  testid?: string;
} = $props();

const id = $props.id();
</script>

<section class="empty" class:centred data-testid={testid} aria-labelledby={`${id}-headline`}>
  <h2 id={`${id}-headline`}>{headline}</h2>
  {#if line}<p>{line}</p>{/if}
  {#if actions.length > 0}
    <ul>
      {#each actions as action (action.label)}
        <li>
          <button type="button" onclick={() => action.run()}>
            <span class="label">{action.label}</span>
            {#if action.keys}<kbd>{action.keys}</kbd>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .empty {
    padding: 16px;
    color: var(--text-2);
  }
  .centred {
    max-width: 340px;
    margin: 18vh auto 0;
    text-align: center;
  }
  h2 {
    margin: 0 0 4px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }
  p {
    margin: 0;
    font-size: 13px;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }
  /* Quiet buttons: the area's one primary is elsewhere. */
  button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  button:hover {
    background: var(--s3);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .empty:not(.centred) ul {
    margin-inline: -8px;
  }
  kbd {
    font: 12px var(--font);
    color: var(--text-2);
  }
  @media (forced-colors: active) {
    button {
      border: 1px solid ButtonText;
    }
  }
</style>
