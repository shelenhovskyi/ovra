<script lang="ts">
// "Also change": other projects one agent in its own copy changes in the same
// task (`docs/specs/multi-repo.md`). Shown only when Settings > Agents turns
// it on; each picked project is a chip with its own remove button.
import type { Project, ProjectId } from "../lib/protocol";
import { useOvra } from "../shell/context";

let {
  project,
  picked = $bindable([]),
}: {
  /** The task's own project, never offered. */
  project: ProjectId;
  picked?: ProjectId[];
} = $props();

/** At most this many other repositories in one task, as the core allows. */
const MAX = 3;

const { transport } = useOvra();
let projects = $state<Project[]>([]);

$effect(() => {
  let gone = false;
  transport
    .call({ method: "projects.list" })
    .then(({ data }) => {
      if (!gone) projects = data;
    })
    .catch(() => {
      // Nothing to offer: the picker stays hidden.
    });
  return () => {
    gone = true;
  };
});

const others = $derived(projects.filter((p) => p.id !== project));
const offered = $derived(others.filter((p) => !picked.includes(p.id)));
const name = (id: ProjectId) => projects.find((p) => p.id === id)?.name ?? id;

function add(id: string): void {
  if (id && !picked.includes(id) && picked.length < MAX) picked = [...picked, id];
}

function remove(id: ProjectId): void {
  picked = picked.filter((p) => p !== id);
}
</script>

{#if others.length > 0}
  <span class="linked" data-testid="linked-repos">
    {#each picked as id (id)}
      <span class="chip">
        {name(id)}
        <button type="button" class="remove" aria-label={`Do not change ${name(id)}`} onclick={() => remove(id)}>×</button>
      </span>
    {/each}
    {#if picked.length < MAX && offered.length > 0}
      <label class="agent-picker" title="Other repositories this agent changes too, each in its own copy">
        <span class="visually-hidden">Also change another repository</span>
        <select
          value=""
          onchange={(event) => {
            add(event.currentTarget.value);
            event.currentTarget.value = "";
          }}
        >
          <option value="">Also change…</option>
          {#each offered as other (other.id)}
            <option value={other.id}>{other.name}</option>
          {/each}
        </select>
      </label>
    {/if}
  </span>
{/if}

<style>
  .linked {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: var(--ctl-lg);
    padding: 0 4px 0 8px;
    font-size: var(--fs-ui);
    color: var(--text);
    background: var(--s3);
    border-radius: var(--r-control);
  }
  .remove {
    width: var(--ctl-sm);
    height: var(--ctl-sm);
    padding: 0;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--r-control);
  }
  .remove:hover {
    color: var(--text);
    background: var(--s2);
  }
  .agent-picker select {
    max-width: 100%;
    height: var(--ctl-lg);
    padding: 0 8px;
    font: inherit;
    font-size: var(--fs-ui);
    color: var(--text-2);
    background: var(--s3);
    border: none;
    border-radius: var(--r-control);
  }
  @media (forced-colors: active) {
    .agent-picker select,
    .chip {
      border: 1px solid ButtonText;
    }
  }
</style>
