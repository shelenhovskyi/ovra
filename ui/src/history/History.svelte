<script lang="ts">
// History, a main-area view (docs/specs/task-history.md): agent sessions of
// every project, newest activity first, grouped by day, with one search
// field and a project menu. Pressing a row hands the session to `onopen`,
// which shows it in the Agents panel, where an ended one can be continued.
// Nothing here starts an agent.
import { untrack } from "svelte";
import { statusLabel } from "../agents/status";
import type { HistoryEntry, Project, ProjectId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import EmptyState from "../shell/EmptyState.svelte";
import ViewFrame from "../shell/ViewFrame.svelte";
import { byDay, timeLabel } from "./days";

let { onopen }: { onopen: (entry: HistoryEntry) => void } = $props();

const { transport } = useOvra();

/** How long typing pauses before the search runs. */
const PAUSE_MS = 150;

let query = $state("");
let project = $state<ProjectId | "">("");
let projects = $state<Project[]>([]);
let entries = $state<HistoryEntry[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
/** The latest search: older replies are stale. */
let asked = 0;

const names = $derived(new Map(projects.map((p) => [p.id, p.name])));
const groups = $derived(byDay(entries, Date.now()));
const searching = $derived(query.trim() !== "");

$effect(() => {
  transport
    .call({ method: "projects.list" })
    .then(({ data }) => (projects = data))
    .catch(() => {});
});

$effect(() => {
  const words = query;
  const only = project;
  const timer = setTimeout(() => untrack(() => void search(words, only)), words === "" ? 0 : PAUSE_MS);
  return () => clearTimeout(timer);
});

async function search(words: string, only: ProjectId | ""): Promise<void> {
  const mine = ++asked;
  loading = true;
  try {
    const { data } = await transport.call({
      method: "agents.history",
      params: { query: words, project: only === "" ? null : only },
    });
    if (mine !== asked) return;
    entries = data;
    error = null;
  } catch (err) {
    if (mine === asked) error = err instanceof Error ? err.message : String(err);
  } finally {
    if (mine === asked) loading = false;
  }
}
</script>

<ViewFrame name="history" testid="history">
  {#snippet tools()}
    <input
      type="search"
      class="query"
      placeholder="Search past tasks"
      aria-label="Search past tasks"
      data-testid="history-search"
      bind:value={query}
    />
    {#if projects.length > 1}
      <select aria-label="Project" data-testid="history-project" bind:value={project}>
        <option value="">All projects</option>
        {#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    {/if}
  {/snippet}
  <div class="history">
    {#if error}
      <p class="error" role="alert">History could not be read: {error}</p>
    {/if}
    {#if entries.length === 0 && loading}
      <p class="muted note">Loading…</p>
    {:else if entries.length === 0 && !searching && project === ""}
      <EmptyState
        centred
        testid="history-empty"
        headline="No past tasks yet"
        line="Tasks you give agents appear here, from every project."
      />
    {:else if entries.length === 0}
      <p class="muted note" data-testid="history-empty">No tasks match.</p>
    {:else}
      {#each groups as group (group.label)}
        <h3>{group.label}</h3>
        <ul aria-label={group.label}>
          {#each group.entries as entry (entry.session.id)}
            {@const s = entry.session}
            <li>
              <button type="button" class="row" data-testid="history-row" onclick={() => onopen(entry)}>
                <span class="line">
                  <strong class="title">{s.title || "Untitled task"}</strong>
                  <span class="when">{timeLabel(s.active_ms ?? s.started_ms)}</span>
                </span>
                <span class="meta">
                  {names.get(s.project) ?? "Removed project"} · {statusLabel(s.status)}
                </span>
                {#if entry.snippet}
                  {@const t = entry.snippet}
                  <span class="snippet" data-testid="history-snippet"
                    >{t.text.slice(0, t.from)}<mark>{t.text.slice(t.from, t.to)}</mark>{t.text.slice(t.to)}</span
                  >
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/each}
      {#if entries.length >= 200}
        <p class="muted note">Showing the newest 200. Search to find older ones.</p>
      {/if}
    {/if}
  </div>
</ViewFrame>

<style>
  .history {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 4px 20px 16px;
  }
  .query {
    flex: 1;
    max-width: 360px;
  }
  input,
  select {
    font: inherit;
    padding: 5px 10px;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-control);
  }
  .muted {
    color: var(--text-2);
    font: var(--t-small);
  }
  .note {
    margin: 4px 0 8px;
  }
  .error {
    color: var(--danger);
    margin: 6px 0;
  }
  h3 {
    margin: 14px 0 6px;
    font: var(--t-small);
    font-weight: 600;
    color: var(--text-2);
  }
  ul {
    list-style: none;
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
  }
  .row {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: 8px 12px;
    text-align: start;
    font: inherit;
    color: var(--text);
    background: var(--s1);
    border: 0;
    border-radius: var(--r-md);
    cursor: default;
  }
  .row:hover {
    background: var(--s3);
  }
  .line {
    display: flex;
    gap: 12px;
    align-items: baseline;
    min-width: 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .when,
  .meta {
    color: var(--text-2);
    font: var(--t-small);
  }
  .meta,
  .snippet {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .snippet {
    color: var(--text-2);
    font: var(--t-small);
  }
  mark {
    color: var(--text);
    background: var(--accent-soft);
    border-radius: 2px;
  }
</style>
