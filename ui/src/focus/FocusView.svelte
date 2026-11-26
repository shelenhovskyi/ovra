<script lang="ts">
// Focus (docs/specs/focus-mode.md): the agents' work without the IDE. The
// shell places the approval cards in the "Waiting on you" column; this adds
// the header, the reviews that wait, and what is in progress, next and
// done. Its root is `display: contents`, so its parts sit in the shell's
// Focus grid beside the approvals.
import { tick } from "svelte";
import { agentSessions, openReviews } from "../agents/sessions.svelte";
import { approvalQueue } from "../approvals/queue.svelte";
import { listCommands } from "../keys/commands";
import type { SessionId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { focusMode } from "./mode.svelte";
import { countLine, type FocusRow, focusSections } from "./model";

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const approvals = approvalQueue(transport);

const sections = $derived(workspace.project ? focusSections(sessions, workspace.project.id) : null);
const nothingWaits = $derived(approvals.pending.length === 0 && (sections?.reviews.length ?? 0) === 0);

function filesLine(id: SessionId): string {
  const n = sessions.changes(id).length;
  return n === 1 ? "1 file changed" : `${n} files changed`;
}

function seeChanges(id: SessionId): void {
  focusMode.set(false);
  openReviews([id]);
}

async function newTask(): Promise<void> {
  focusMode.set(false);
  await tick();
  void listCommands()
    .find((c) => c.id === "agents.new_session")
    ?.run();
}
</script>

{#snippet rows(name: string, list: FocusRow[], mark: string)}
  <section class="part" aria-label={name} data-testid={`focus-${mark}`}>
    <h3>{name}{#if list.length}<span class="n">{list.length}</span>{/if}</h3>
    {#if list.length}
      <ul>
        {#each list as row (row.id)}
          <li><span class="mark {mark}" aria-hidden="true"></span><span class="name">{row.name}</span><span
              class="status">{row.status}</span
            ></li>
        {/each}
      </ul>
    {:else}
      <p class="none">None</p>
    {/if}
  </section>
{/snippet}

<div class="focus-root">
  <header class="focus-head" data-testid="focus-head">
    <h2>{workspace.project?.name ?? "No project"}</h2>
    {#if sections}<p>{countLine(sections)}</p>{/if}
  </header>

  <section class="focus-waiting" aria-label="Changes to review">
    {#if nothingWaits}
      <p class="quiet" data-testid="focus-nothing">Nothing waits for you.</p>
    {/if}
    {#each sections?.reviews ?? [] as id (id)}
      {@const summary = sessions.reviews.get(id)?.summary}
      <article class="card" data-testid="focus-review">
        <p class="who">Ready to review</p>
        <h3>{sessions.name(id)}</h3>
        {#if summary}<p class="summary">{summary}</p>{/if}
        <p class="facts">{filesLine(id)}</p>
        <div class="acts">
          <button type="button" class="primary" onclick={() => seeChanges(id)}>See changes</button>
        </div>
      </article>
    {/each}
  </section>

  <div class="focus-status">
    {#if sections}
      {@render rows("In progress", sections.progress, "progress")}
      {@render rows("Next", sections.next, "next")}
      {@render rows("Done", sections.done, "done")}
    {/if}
    <button type="button" class="new" onclick={newTask} data-testid="focus-new-task">New task…</button>
  </div>
</div>

<style>
  .focus-root {
    display: contents;
  }
  .focus-head {
    grid-area: head;
    padding: 0 2px;
  }
  .focus-head h2 {
    margin: 0;
    font-size: 17px;
    line-height: 24px;
    font-weight: 600;
  }
  .focus-head p {
    margin: 2px 0 0;
    color: var(--text-2);
  }
  .focus-waiting {
    grid-area: reviews;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .quiet {
    margin: 0;
    padding: 18px;
    border-radius: var(--r-lg);
    background: var(--s2);
    text-align: center;
    color: var(--text-2);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    border-radius: var(--r-lg);
    background: var(--s2);
  }
  .card p,
  .card h3 {
    margin: 0;
  }
  .card h3 {
    font-size: 14px;
    line-height: 20px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .who,
  .facts {
    font-size: 12px;
    color: var(--text-3);
  }
  .summary {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .acts {
    display: flex;
    gap: 8px;
  }
  .focus-status {
    grid-area: status;
    display: flex;
    flex-direction: column;
    gap: 22px;
    min-width: 0;
  }
  .part h3 {
    display: flex;
    gap: 8px;
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
  }
  .n {
    font-weight: 400;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    gap: 0 10px;
    padding: 7px 2px;
  }
  li + li {
    box-shadow: 0 -1px 0 var(--separator);
  }
  .mark {
    width: 8px;
    height: 8px;
    margin-top: 6px;
    border-radius: 50%;
    justify-self: center;
  }
  .mark.progress {
    background: var(--accent);
  }
  .mark.next {
    box-shadow: inset 0 0 0 1.5px var(--text-3);
  }
  .mark.done {
    background: var(--ok);
  }
  .name {
    overflow-wrap: anywhere;
  }
  .status {
    grid-column: 2;
    font-size: 12px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .none {
    margin: 0;
    padding: 4px 2px;
    color: var(--text-3);
  }
  .new {
    align-self: flex-start;
  }
  button {
    min-height: var(--control-h);
    padding: 0 12px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }
  button:hover {
    background: var(--s4);
  }
  .primary,
  .primary:hover {
    color: var(--accent-ink);
    background: var(--accent);
    font-weight: 500;
  }
</style>
