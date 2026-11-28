<script lang="ts">
// A question an agent asked the person on its task's board, in Focus. The
// answer is text sent back to the board; it allows nothing.
import { dropZone, insidePaths, pastedFiles } from "../agents/attach";
import { taskBoards } from "../board/boards.svelte";
import { authorName } from "../board/describe";
import type { BoardEntry, TaskId } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { useOvra } from "../shell/context";

let { task, taskName, question }: { task: TaskId; taskName: string; question: BoardEntry } = $props();

const { transport, workspace } = useOvra();
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

let form = $state<HTMLFormElement>();
let field = $state<HTMLTextAreaElement>();
/** Files are dragged over the card. */
let fileOver = $state(false);

// A project file dropped on the card goes into the answer as its path: the
// answer is text, and the agent reads the file itself. Files from elsewhere
// would need a copy in the project, which the asking worker's own worktree
// does not get, so they go in a task's reply box instead.
const ELSEWHERE = "Add files from elsewhere in the task's reply box: here only the project's files can go.";

$effect(() => {
  const box = form;
  const input = field;
  const project = workspace.project;
  if (!box || !input || !project) return;
  const off = dropZone(box, transport, {
    over: (on) => (fileOver = on),
    window: (drop) => {
      const paths = insidePaths(project.root, drop);
      if (paths) void insert(project.id, paths);
      else error = ELSEWHERE;
    },
    rows: (paths) => void insert(project.id, paths),
    files: () => (error = ELSEWHERE),
  });
  const paste = (event: ClipboardEvent) => {
    if (pastedFiles(event.clipboardData).length === 0) return;
    event.preventDefault();
    error = ELSEWHERE;
  };
  input.addEventListener("paste", paste);
  return () => {
    off();
    input.removeEventListener("paste", paste);
  };
});

/** Puts project paths into the answer, unless one looks like a secret store (as an amber chip would). */
async function insert(project: string, paths: string[]): Promise<void> {
  error = null;
  try {
    const items = paths.map((path) => ({ kind: "file" as const, path }));
    const checked = await transport.call({ method: "context.check", params: { project, items } });
    const risky = paths.filter((_, i) => checked.data.notes[i] != null);
    if (risky.length > 0) {
      error = `${risky.join(", ")} may hold secrets: type the path if you mean it.`;
      return;
    }
    const gap = text === "" || /\s$/.test(text) ? "" : " ";
    text = `${text}${gap}${paths.join(" ")} `;
    field?.focus();
  } catch (e) {
    error = e instanceof CoreError ? e.message : e instanceof Error ? e.message : String(e);
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
  bind:this={form}
  class="card"
  class:file-over={fileOver}
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
    bind:this={field}
    bind:value={text}
    {onkeydown}
  ></textarea>
  <div class="acts">
    <button type="submit" class:primary={!options.length} disabled={busy || !text.trim()}>Answer</button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</form>

<style>
  .card.file-over {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
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
    font-size: var(--fs-ui-sm);
    color: var(--text-2);
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
    border-radius: var(--r-control);
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
    font-size: var(--fs-ui);
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
