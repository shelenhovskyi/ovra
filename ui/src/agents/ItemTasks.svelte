<script lang="ts">
// An answer that lists numbered items: the person ticks some and one press
// gives each its own agent in its own copy, so they run side by side and
// come back one by one in Reviews. Beyond the agent limit they wait their
// turn. Nothing starts without the press.
import { type ListItem, numbersText } from "./items";

let {
  items,
  free,
  why,
  started,
  onstart,
}: {
  items: ListItem[];
  /** Places free under the agent limit now; undefined while unknown. */
  free: number | undefined;
  /** Why the items cannot be started now, or null when they can. */
  why: string | null;
  /** The numbers already started from this answer, or null. */
  started: number[] | null;
  onstart: (ns: number[]) => void;
} = $props();

let picked = $state<number[]>([]);
const ordered = $derived(items.map((i) => i.n).filter((n) => picked.includes(n)));

function toggle(n: number): void {
  picked = picked.includes(n) ? picked.filter((p) => p !== n) : [...picked, n];
}

const waits = $derived(free === undefined ? 0 : Math.max(0, ordered.length - free));
const line = $derived(
  started
    ? `Started ${started.length === 1 ? "item" : "items"} ${numbersText(started)}, each with its own agent. Their changes come to Reviews.`
    : (why ??
        (waits > 0
          ? `${ordered.length - waits} start now, ${waits} ${waits === 1 ? "waits" : "wait"} for a free agent.`
          : "Each picked item gets its own agent and its own copy of the project.")),
);
</script>

<section class="items" aria-labelledby="agent-items-heading" data-testid="agent-items">
  <h4 id="agent-items-heading">Do items as separate tasks</h4>
  {#if !started}
    <ul aria-labelledby="agent-items-heading">
      {#each items as item (item.n)}
        <li>
          <label title={item.title}>
            <input type="checkbox" checked={picked.includes(item.n)} onchange={() => toggle(item.n)} />
            <span class="n">{item.n}.</span>
            <span class="title">{item.title}</span>
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="row">
    <span class="line" data-testid="agent-items-line">{line}</span>
    {#if !started}
      <button
        type="button"
        data-testid="agent-items-start"
        disabled={ordered.length === 0 || why !== null}
        onclick={() => onstart(ordered)}>{ordered.length === 0 ? "Pick items" : `Do ${numbersText(ordered)}`}</button
      >
    {/if}
  </div>
</section>

<style>
  /* As the agents panel's other outcome cards. */
  .items {
    margin: 0 8px 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  h4 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 168px;
    overflow-y: auto;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 2px 0;
    cursor: pointer;
  }
  .n {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .line {
    color: var(--text-2);
    font: var(--t-small);
  }
  button {
    flex: none;
    height: var(--ctl);
    padding: 0 10px;
    font: inherit;
    font-size: var(--fs-ui);
    color: var(--text);
    background: var(--s3);
    border: none;
    border-radius: var(--r-control);
    cursor: pointer;
  }
  button:disabled {
    color: var(--text-2);
    cursor: default;
  }
</style>
