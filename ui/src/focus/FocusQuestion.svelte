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
  <label class="visually-hidden" for={`focus-answer-${task}-${question.id}`}>Your answer</label>
  <textarea
    id={`focus-answer-${task}-${question.id}`}
    rows="2"
    placeholder="Your answer"
    bind:value={text}
    {onkeydown}
  ></textarea>
  <div class="acts">
    <button type="submit" class="primary" disabled={busy || !text.trim()}>Answer</button>
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
