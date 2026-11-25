<script lang="ts">
// Settings > Claude: how many agents may work at once in a project, or with
// `research` how many research agents (ADR 0011), which have their own cap.
// The core checks the range and keeps the choice; the field shows what it kept.
import { untrack } from "svelte";
import type { AgentLimit } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

const { research = false }: { research?: boolean } = $props();
const { transport } = useOvra();
const changed = $derived(research ? "agents.research_limit_changed" : "agents.limit_changed");
const id = $derived(research ? "research-limit" : "agent-limit");

let limit = $state<AgentLimit | null>(null);
let error = $state<string | null>(null);

$effect(() =>
  untrack(() => {
    void load();
    return transport.onEvent((event) => {
      if (event.type === changed) {
        limit = event.data;
        error = null;
      }
    });
  }),
);

async function load(): Promise<void> {
  try {
    limit = research
      ? (await transport.call({ method: "agents.research_limit" })).data
      : (await transport.call({ method: "agents.limit" })).data;
  } catch {
    // A front end without agents has no limit to show.
  }
}

async function save(input: HTMLInputElement): Promise<void> {
  const value = Number(input.value);
  error = null;
  if (!Number.isInteger(value) || input.value.trim() === "") {
    error = limit ? `Choose a whole number from ${limit.min} to ${limit.max}.` : null;
    if (limit) input.value = String(limit.limit);
    return;
  }
  try {
    limit = research
      ? (await transport.call({ method: "agents.set_research_limit", params: { limit: value } })).data
      : (await transport.call({ method: "agents.set_limit", params: { limit: value } })).data;
  } catch (err) {
    error = err instanceof CoreError ? err.error.message : String(err);
  }
  if (limit) input.value = String(limit.limit);
}
</script>

{#if limit}
  <div class="limit" data-testid={id}>
    <label class="field">
      <span>{research ? "Research agents at once" : "Agents working at once"}</span>
      <input
        type="number"
        min={limit.min}
        max={limit.max}
        step="1"
        value={limit.limit}
        aria-describedby="{id}-why"
        aria-invalid={error !== null}
        onchange={(e) => void save(e.currentTarget)}
      />
    </label>
    <p class="hint" id="{id}-why">
      {#if research}
        From {limit.min} to {limit.max} in each project ({limit.default} by default; 0 turns research off). Research agents
        only read the project and search the web, so they do not count against the agents above, but each one still uses
        your Claude usage limits.
      {:else}
      From {limit.min} to {limit.max} in each project ({limit.default} by default). More agents finish independent work
      sooner, but each one uses your Claude usage limits and disk space for its own copy of the project, and leaves
      more changes for you to review. Beyond the limit, workers wait their turn and new sessions cannot start.
      {/if}
    </p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .limit {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 480px;
    margin-top: var(--sp-3);
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .field input {
    width: 5em;
  }
  p {
    margin: 0;
  }
  .hint {
    font: var(--t-small);
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
</style>
