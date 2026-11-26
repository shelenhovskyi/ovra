<script lang="ts">
// A task's board: one line on who holds what, the questions waiting on the
// person with a box to answer each, every entry in the order it was written,
// and a box for the person's own note. Questions show here and in the task
// list's "Answer" chip only: no toast, no sound.
import type { EntryId, TaskId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";
import { taskBoards } from "./boards.svelte";
import { authorName, entryHead, entryText, summary } from "./describe";

/** `id` and `labelledby`: the tab panel's own id and its tab's. */
let { task, id, labelledby }: { task: TaskId; id?: string; labelledby?: string } = $props();

const { transport } = useOvra();
const boards = taskBoards(transport);

$effect(() => boards.watch(task));

const entries = $derived(boards.entries.get(task) ?? []);
const open = $derived(boards.openForPerson(task));
const answers = $state<Record<EntryId, string>>({});
let note = $state("");
let busy = $state(false);
let error = $state<string | null>(null);

async function run(write: () => Promise<void>): Promise<boolean> {
  busy = true;
  error = null;
  try {
    await write();
    return true;
  } catch (e) {
    error = e instanceof CoreError ? e.message : String(e);
    return false;
  } finally {
    busy = false;
  }
}

async function answer(question: EntryId): Promise<void> {
  const text = answers[question]?.trim();
  if (!text) return;
  if (await run(() => boards.answer(task, question, text))) delete answers[question];
}

async function post(): Promise<void> {
  const text = note.trim();
  if (!text) return;
  if (await run(() => boards.post(task, text))) note = "";
}

/** Enter sends, Shift+Enter starts a new line, as in the reply box. */
function onEnter(event: KeyboardEvent, send: () => void): void {
  if (event.code !== "Enter" && event.code !== "NumpadEnter") return;
  if (event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  send();
}
</script>

<section
  class="board"
  role={labelledby ? "tabpanel" : undefined}
  {id}
  aria-label={labelledby ? undefined : "Board"}
  aria-labelledby={labelledby}
  data-testid="task-board"
>
  <p class="summary" data-testid="board-summary">{summary(entries, open.length)}</p>
  {#each open as question (question.id)}
    <form
      class="ask"
      data-testid="board-question"
      onsubmit={(event) => {
        event.preventDefault();
        void answer(question.id);
      }}
    >
      <p><span class="who">{authorName(question.author)} asks you</span>{entryText(question)}</p>
      <label class="visually-hidden" for={`answer-${question.id}`}>Your answer</label>
      <textarea
        id={`answer-${question.id}`}
        rows="2"
        placeholder="Your answer"
        bind:value={answers[question.id]}
        onkeydown={(event) => onEnter(event, () => void answer(question.id))}
      ></textarea>
      <button type="submit" class="primary" disabled={busy || !answers[question.id]?.trim()}>Answer</button>
    </form>
  {/each}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <ol class="entries" aria-label="Entries">
    {#each entries as entry (entry.id)}
      <li data-testid="board-entry" data-kind={entry.body.kind}>
        <span class="who">{authorName(entry.author)} · {entryHead(entry, entries)}</span>
        <p>{entryText(entry)}</p>
      </li>
    {/each}
  </ol>
  <form
    class="note"
    onsubmit={(event) => {
      event.preventDefault();
      void post();
    }}
  >
    <label class="visually-hidden" for={`board-note-${task}`}>Note for the agents</label>
    <textarea
      id={`board-note-${task}`}
      rows="2"
      placeholder="Note for the agents"
      bind:value={note}
      onkeydown={(event) => onEnter(event, () => void post())}
    ></textarea>
    <button type="submit" class="quiet" disabled={busy || !note.trim()}>Post</button>
  </form>
</section>

<style>
  .board {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 16px 8px;
  }
  p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .summary,
  .who {
    font-size: 13px;
    color: var(--text-2);
  }
  .who {
    display: block;
  }
  .ask {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 12px;
    background: var(--s2);
    border-radius: var(--radius);
  }
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .note {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    color: var(--text);
    background: var(--s1);
    border: 1px solid var(--separator);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    resize: vertical;
  }
  button {
    font: inherit;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    flex: none;
  }
  button:disabled {
    cursor: default;
  }
  .primary {
    height: 32px;
    padding: 0 14px;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .primary:disabled {
    color: var(--text-2);
    background: var(--s3);
  }
  .quiet {
    height: 28px;
    padding: 0 8px;
    font-size: 13px;
    color: var(--text-2);
    background: none;
  }
  .quiet:hover:not(:disabled) {
    color: var(--text);
    background: var(--s3);
  }
  .quiet:disabled {
    opacity: 0.5;
  }
  .error {
    font-size: 13px;
    color: var(--danger);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
