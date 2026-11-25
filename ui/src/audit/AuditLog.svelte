<script lang="ts">
// The audit log, a main-area view: the open project's entries, newest
// first, updated live. It opens on the newest page; "Load older" at the end
// of the list reads the page before. It only reads: the core offers no way
// to edit or remove an entry. The shell opens it ("Audit log" command).
import { tick, untrack } from "svelte";
import { agentSessions } from "../agents/sessions.svelte";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import ViewFrame from "../shell/ViewFrame.svelte";
import { describe, FILTERS, type Filter, shows, when } from "./describe";
import { AuditTrail } from "./trail.svelte";

const { transport, workspace } = useOvra();
const trail = new AuditTrail(transport);
const sessions = agentSessions(transport);

let filter = $state<Filter>("all");
let refusedOnly = $state(false);
let list = $state<HTMLElement>();
/** Read out when older entries arrive. */
let olderNote = $state("");

const shown = $derived(
  trail.entries
    .filter((entry) => shows(filter, refusedOnly, entry))
    .reverse()
    .map((entry) => ({ entry, ...describe(entry) })),
);

$effect(() => () => trail.dispose());

// Read the log, and again for another project.
$effect(() => {
  const project = workspace.project?.id ?? null;
  // Only the project: reading the log must not run it again.
  untrack(() => void trail.show(project));
});

async function loadOlder(): Promise<void> {
  const added = await trail.loadOlder();
  if (added > 0) olderNote = `${added} older ${added === 1 ? "entry" : "entries"} loaded.`;
  await tick();
  // At the start of the log the button goes; focus stays in the view.
  const view = list?.closest("section");
  if (view && !view.contains(document.activeElement)) view.querySelector<HTMLElement>("h2")?.focus();
}

function who(session: string | null): string {
  return session ? sessions.name(session) : "Project";
}
</script>

<ViewFrame name="audit" testid="audit">
  {#snippet tools()}
    <span class="muted">{workspace.project?.name ?? "No project"}</span>
    {#if trail.entries.length > 0 || trail.loading}
      <!-- Nothing to filter in an empty log. -->
      <label>
        Show
        <select bind:value={filter} data-testid="audit-filter">
          {#each Object.entries(FILTERS) as [value, { label }] (value)}
            <option {value}>{label}</option>
          {/each}
        </select>
      </label>
      <label><input type="checkbox" bind:checked={refusedOnly} data-testid="audit-refused" /> Refused only</label>
    {/if}
  {/snippet}
  <div class="log" bind:this={list}>
    {#if trail.error}
      <p class="error" role="alert">The audit log could not be read: {trail.error}</p>
    {/if}
    {#if !workspace.project}
      <EmptyState centred headline="No project open" line="Open a project to see what its agents did." />
    {:else if shown.length === 0 && trail.loading}
      <p class="muted note" data-testid="audit-empty">Loading the audit log…</p>
    {:else if shown.length === 0 && trail.entries.length === 0}
      <EmptyState
        centred
        testid="audit-empty"
        headline="Nothing recorded yet"
        line="Agent starts, approvals and tool calls appear here."
      />
    {:else if shown.length === 0}
      <p class="muted note" data-testid="audit-empty">No entries match.</p>
    {:else}
      <p class="muted note">What agents did and who allowed it. Append-only, without file contents or tokens.</p>
      <ol class="entries" aria-label="Audit entries, newest first">
        {#each shown as row (row.entry.seq)}
          <li class={row.tone} data-testid="audit-entry">
            <div class="line">
              <time datetime={new Date(row.entry.at_ms).toISOString()}>{when(row.entry.at_ms)}</time>
              <span class="who">{who(row.entry.session)}</span>
              <strong>{row.title}</strong>
            </div>
            <p class="detail">{row.detail}</p>
            {#if row.input}
              <pre class="input">{row.input}</pre>
            {/if}
            {#if row.raw}
              <details>
                <summary>Details</summary>
                <pre>{row.raw}</pre>
              </details>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
    {#if workspace.project && trail.older}
      <p class="more">
        <span class="muted">Older entries are not loaded yet.</span>
        <button type="button" data-testid="audit-older" aria-busy={trail.loadingOlder} onclick={() => void loadOlder()}>
          {trail.loadingOlder ? "Loading…" : "Load older"}
        </button>
      </p>
    {/if}
    <p class="visually-hidden" role="status" data-testid="audit-older-note">{olderNote}</p>
  </div>
</ViewFrame>

<style>
  .log {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
  .muted,
  label {
    color: var(--text-2);
    font: var(--t-small);
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .note {
    margin: 4px 20px 8px;
  }
  button,
  input,
  select {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  button {
    font-weight: 500;
    cursor: default;
  }
  button:hover {
    background: var(--s4);
  }
  button[aria-busy="true"] {
    cursor: progress;
    opacity: 0.6;
  }
  summary {
    width: fit-content;
    cursor: default;
  }
  .error {
    color: var(--danger);
    margin: 6px 20px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .entries {
    list-style: none;
    display: grid;
    align-content: start;
    gap: 4px;
    margin: 0;
    padding: 4px 20px 16px;
  }
  li {
    padding: 8px 12px;
    border-radius: var(--r-md);
    background: var(--s1);
  }
  li.refused strong {
    color: var(--danger);
  }
  li.warn strong {
    color: var(--warn);
  }
  .more {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    padding: 8px 20px 14px;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: baseline;
  }
  strong {
    font-weight: 600;
  }
  time,
  .who {
    color: var(--text-2);
    font: var(--t-small);
    font-variant-numeric: tabular-nums;
  }
  .detail {
    margin: 2px 0 0;
    color: var(--text-2);
  }
  details {
    margin-top: 2px;
    font: var(--t-small);
    color: var(--text-2);
  }
  pre {
    margin: 4px 0 0;
    max-height: 8em;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: 13px / 18px var(--mono);
    color: var(--text);
    background: var(--s0);
    padding: 6px 8px;
    border-radius: var(--r-sm);
  }
  @media (forced-colors: active) {
    button,
    input,
    select {
      border: 1px solid ButtonText;
    }
  }
</style>
