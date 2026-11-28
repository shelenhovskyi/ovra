<script lang="ts" module>
import type { IconName } from "./icons";

/** A button of an empty state: a verb, an icon, the shortcut that does the same, what it runs. */
export type EmptyAction = { label: string; icon?: IconName; keys?: string | null; run: () => void };
</script>

<script lang="ts">
import Icon from "./Icon.svelte";

// What a panel shows when it has nothing to show (docs/design/patterns.md,
// "Empty states"): a headline, one line, and the next steps as quiet filled
// buttons with their keys, so they read as buttons before any hover. Centred in the main area, top-left in side panels.
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
            {#if action.icon}<Icon name={action.icon} small />{/if}
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
    font-size: var(--fs-ui);
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }
  /* Quiet buttons: a soft fill and hairline at rest, the area's one primary is elsewhere. */
  button {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    height: var(--ctl-lg);
    padding: 0 10px;
    font: inherit;
    font-size: var(--fs-ui);
    color: var(--text);
    text-align: left;
    background: var(--s3);
    border: 1px solid var(--separator);
    border-radius: var(--r-control);
    cursor: pointer;
  }
  button:hover {
    background: var(--s4);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  button :global(.icon) {
    color: var(--text-2);
  }
  .label {
    flex: 1;
  }
  /* Side panels: buttons as wide as their words, under the headline. */
  .empty:not(.centred) ul {
    align-items: flex-start;
  }
  .empty:not(.centred) button {
    width: auto;
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
