<script lang="ts">
// A question an agent asked the person on its task's board, in Focus. The
// answer is text sent back to the board; it allows nothing.
import { taskBoards } from "../board/boards.svelte";
import { authorName } from "../board/describe";
import type { BoardEntry, TaskId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

let { task, taskName, question }: { task: TaskId; taskName: string; question: BoardEntry } = $props();

const { transport } = useOvra();
const boards = taskBoards(transport);

let text = $state("");
let busy = $state(false);
let error = $state<string | null>(null);

const asked = $derived(question.body.kind === "question" ? question.body.text : "");
// Answers to pick, from the agent. The recommended one is marked, never
// picked for the person: a tap sends that option's exact text.
const options = $derived(question.body.kind === "question" ? (question.body.options ?? []) : []);
const recommended = $derived(question.body.kind === "question" ? question.body.recommended : undefined);
const reason = $derived(question.body.kind === "question" ? question.body.reason : undefined);

async function send(answer: string): Promise<void> {
  const value = answer.trim();
  if (!value || busy) return;
  busy = true;
  error = null;
  try {
    await boards.answer(task, question.id, value);
    text = "";
  } catch (e) {
    error = e instanceof CoreError ? e.message : String(e);
  } finally {
    busy = false;
  }
}

/** Enter sends, Shift+Enter starts a new line, as on the board. */
function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Enter" && event.code !== "NumpadEnter") return;
  if (event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  void send(text);
}
</script>

<form
  class="card"
  data-testid="focus-question"
  onsubmit={(event) => {
    event.preventDefault();
    void send(text);
  }}
>
  <p class="who">Question from {authorName(question.author)} · {taskName}</p>
  <h3>{asked}</h3>
  {#if options.length}
    <div class="options" role="group" aria-label="Answers">
      {#each options as option, i (i)}
        <button
          type="button"
          class:recommended={recommended === i}
          disabled={busy}
          data-testid="focus-option"
          onclick={() => void send(option)}
          >{option}{#if recommended === i}<span class="tag">Recommended</span>{/if}</button
        >
      {/each}
    </div>
    {#if reason}<p class="why" data-testid="focus-reason">{reason}</p>{/if}
  {/if}
  <label class="visually-hidden" for={`focus-answer-${task}-${question.id}`}>Your answer</label>
  <textarea
    id={`focus-answer-${task}-${question.id}`}
    rows="2"
    placeholder={options.length ? "Or answer in your words" : "Your answer"}
    bind:value={text}
    {onkeydown}
  ></textarea>
  <div class="acts">
    <button type="submit" class:primary={!options.length} disabled={busy || !text.trim()}>Answer</button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    border-radius: var(--r-lg);
    background: var(--s2);
    box-shadow: inset 0 0 0 1px var(--warn-soft);
  }
  .card p,
  .card h3 {
    margin: 0;
  }
  .who {
    font-size: 12px;
    color: var(--text-3);
  }
  h3 {
    font-size: 14px;
    line-height: 20px;
    font-weight: 600;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 6px 10px;
    color: var(--text);
    background: var(--s1);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    resize: vertical;
    font: inherit;
  }
  .acts {
    display: flex;
    gap: 8px;
  }
  button {
    min-height: var(--control-h);
    padding: 0 12px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
  }
  .primary:not(:disabled) {
    color: var(--accent-ink);
    background: var(--accent);
    font-weight: 500;
  }
  .options {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .options button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    text-align: start;
    overflow-wrap: anywhere;
  }
  .options .recommended {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .tag {
    font-size: 11px;
    color: var(--accent);
    font-weight: 500;
  }
  .why {
    font-size: 13px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .error {
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
