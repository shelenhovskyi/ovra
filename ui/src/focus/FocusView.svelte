<script lang="ts">
// Focus (docs/specs/focus-mode.md): the agents' work without the IDE, in
// one column. The shell places the approval cards under "Waiting on you";
// this adds the header, the questions and reviews that wait, what is in
// progress, next and done, and the field for the next task. Its root is
// `display: contents`, so its parts sit in the shell's Focus grid around
// the approvals.
import { agentSessions, openReviews } from "../agents/sessions.svelte";
import { approvalQueue } from "../approvals/queue.svelte";
import { taskBoards } from "../board/boards.svelte";
import type { BoardEntry, SessionId, TaskId } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { Away, type Moment } from "./away.svelte";
import FocusComposer from "./FocusComposer.svelte";
import FocusQuestion from "./FocusQuestion.svelte";
import { focusMode } from "./mode.svelte";
import { countLine, type FocusRow, focusSections } from "./model";

const { transport, workspace } = useOvra();
const sessions = agentSessions(transport);
const approvals = approvalQueue(transport);
const boards = taskBoards(transport);

const sections = $derived(workspace.project ? focusSections(sessions, workspace.project.id) : null);
// Plans come from transcripts: load those of agents at work that this window has not watched.
const asked = new Set<SessionId>();
$effect(() => {
  for (const row of sections?.progress ?? []) {
    if (sessions.transcripts.has(row.id) || asked.has(row.id)) continue;
    asked.add(row.id);
    void sessions.loadTranscript(row.id).catch(() => {});
  }
});

/** The open project's tasks, by their coordinators: their boards hold the agents' questions to the person. */
const tasks = $derived.by(() => {
  const project = workspace.project?.id;
  if (!project) return [];
  return sessions.shown(project).flatMap((s) => {
    const role = sessions.roles.get(s.id);
    return role?.kind === "coordinator" ? [{ task: role.task, name: s.title || sessions.name(s.id) }] : [];
  });
});
$effect(() => {
  for (const { task } of tasks) boards.watch(task);
});
const questions = $derived(
  tasks.flatMap(({ task, name }) =>
    boards.openForPerson(task).map((question): { task: TaskId; name: string; question: BoardEntry } => ({
      task,
      name,
      question,
    })),
  ),
);

// "Where you left off": after a break, what ended meanwhile and what waits now.
const away = new Away();
const waiting = $derived(approvals.pending.length + questions.length + (sections?.reviews.length ?? 0));
const nothingWaits = $derived(waiting === 0);
const moment = (): Moment => ({ done: sections?.done ?? [], waiting });
const here = () => away.here(Date.now(), moment());
const left = () => away.left(Date.now(), moment());
function visibility(): void {
  if (document.visibilityState === "hidden") left();
  else here();
}

function filesLine(id: SessionId): string {
  const n = sessions.changes(id).length;
  return n === 1 ? "1 file changed" : `${n} files changed`;
}

function seeChanges(id: SessionId): void {
  focusMode.set(false);
  openReviews([id]);
}
</script>

{#snippet rows(name: string, list: FocusRow[], mark: string)}
  {#if list.length}<section class="part" aria-label={name} data-testid={`focus-${mark}`}>
    <h2>{name}{#if list.length}<span class="n">{list.length}</span>{/if}</h2>
    <ul>
        {#each list as row (`${row.id}:${row.step ?? ""}`)}
          <li><span class="mark {mark}" aria-hidden="true"></span><span class="name">{row.name}</span><span
              class="status">{row.status}</span
            ></li>
        {/each}
      </ul>
  </section>{/if}
{/snippet}

<svelte:window onkeydown={here} onpointerdown={here} onfocus={here} onblur={left} />
<svelte:document onvisibilitychange={visibility} />

<div class="focus-root">
  <div class="focus-head">
    <div data-testid="focus-head">
      <!-- Focus is the whole page: the project is its top heading, the
           parts are its sections and each card is under them. -->
      <h1>{workspace.project?.name ?? "No project"}</h1>
      {#if sections}<p>{countLine(sections)}</p>{/if}
    </div>
    <div role="status" class="away-slot">
      {#if away.line}<p class="away" data-testid="focus-away">
          <span>{away.line}</span>
          <button type="button" class="dismiss" onclick={() => away.dismiss()}>Got it</button>
        </p>{/if}
    </div>
  </div>

  <h2 class="focus-label">Waiting on you</h2>

  <section class="focus-waiting" aria-label="Questions and changes to review">
    {#if nothingWaits}
      <p class="quiet" data-testid="focus-nothing">Nothing waits for you.</p>
    {/if}
    {#each questions as { task, name, question } (`${task}:${question.id}`)}
      <FocusQuestion {task} taskName={name} {question} />
    {/each}
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
  </div>

  <div class="focus-compose"><FocusComposer /></div>
</div>

<style>
  .focus-root {
    display: contents;
  }
  .focus-head {
    grid-area: head;
    padding: 0 2px;
  }
  .focus-head h1 {
    margin: 0;
    font-size: 17px;
    line-height: 24px;
    font-weight: 600;
  }
  .focus-head p {
    margin: 2px 0 0;
    color: var(--text-2);
  }
  .focus-head .away {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--r-lg);
    background: var(--s2);
    color: var(--text);
  }
  .away span {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .dismiss {
    flex: none;
  }
  .focus-waiting {
    grid-area: reviews;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .focus-label,
  .part h2 {
    display: flex;
    gap: 8px;
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-2);
  }
  .focus-label {
    grid-area: wait;
  }
  .quiet {
    margin: 0;
    padding: 2px 2px 0;
    color: var(--text-2);
  }
  .focus-compose {
    grid-area: compose;
    position: sticky;
    bottom: 0;
    padding: 8px 0 16px;
    background: var(--s1);
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
    color: var(--text-2);
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
  .part h2 {
    margin-bottom: 6px;
  }
  .n {
    font-weight: 400;
    color: var(--text-2);
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
