<script lang="ts">
// The commit box: what changed, and one Commit button that names what it
// commits. Without staging it commits every change (staging them first);
// with staging, what is staged. Mod+Enter anywhere in the box commits. Commit
// says why it waits only once pressed; a hook's output is shown when the
// commit fails, and a short line names the commit when it is made.
import { tick } from "svelte";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { RepoStatus } from "../lib/protocol";
import type { Assist } from "./assist.svelte";
import { focusNextStep } from "./focus";
import { changedFiles, fileLists, type GitModel } from "./model.svelte";

let { model, status, assist, staging }: { model: GitModel; status: RepoStatus; assist: Assist; staging: boolean } =
  $props();

const shortcut = formatShortcut("Mod+Enter", isApple());
let text = $state("");
/** The person's own text that an agent's draft replaced, for Undo. */
let replaced = $state<string | null>(null);
let box = $state<HTMLTextAreaElement>();
/** Commit was pressed while it could not commit: say why. */
let tried = $state(false);

// An agent drafted a message: it goes in the box, for the person to edit and commit.
$effect(() => {
  const draft = assist.draft;
  if (draft == null) return;
  assist.draft = null;
  replaced = text.trim() && text !== draft ? text : null;
  text = draft;
  void tick().then(() => box?.focus());
});

function undoDraft(): void {
  if (replaced === null) return;
  text = replaced;
  replaced = null;
  box?.focus();
}

/** Files the commit takes: what is staged, or without staging every change. */
const count = $derived(staging ? fileLists(status.files).staged.length : changedFiles(status.files).length);
/** Changes to stage before committing, without staging. */
const unstaged = $derived(
  staging ? [] : fileLists(status.files).changes.flatMap((f) => (f.from ? [f.path, f.from] : [f.path])),
);
const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;
const label = $derived(model.amend ? "Change last commit" : count === 0 ? "Commit" : `Commit ${files(count)}`);
const why = $derived.by(() => {
  if (!model.amend && count === 0) return "Stage the files to commit first.";
  if (!text.trim() && !model.amend) return "Write what you changed first.";
  return null;
});

/** Blanks the reason for a moment, so that its live region reads it out again. */
let repeating = $state(false);

async function commit(): Promise<void> {
  if (model.busy) return;
  if (why) {
    // Say why nothing happened, where it shows, and once more on each press.
    tried = true;
    repeating = true;
    await tick();
    repeating = false;
    if (!text.trim()) box?.focus();
    return;
  }
  if (unstaged.length > 0 && !(await model.stage(unstaged))) return;
  if (await model.commit(text.trim(), model.amend)) {
    text = "";
    replaced = null;
    tried = false;
    model.amend = false;
    // The box may go with the changes: focus goes on to Send or the next change.
    await tick();
    focusNextStep();
  }
}

function cancelAmend(): void {
  model.amend = false;
  box?.focus();
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
  event.preventDefault();
  if (!event.repeat) void commit();
}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="commit" role="group" aria-label="Commit" {onkeydown}>
  {#if model.amend}
    <p class="hint" data-testid="git-amend">
      Changes the last commit: its message{count ? ", and adds these files" : ""}. An empty message keeps the old one.
      <button type="button" class="link" onclick={cancelAmend}>Cancel</button>
    </p>
  {/if}
  <label class="message">
    <span>Commit message</span>
    <textarea
      rows="3"
      spellcheck="true"
      placeholder="What did you change? For example: Fix the login button"
      aria-keyshortcuts={isApple() ? "Meta+Enter" : "Control+Enter"}
      bind:value={text}
      bind:this={box}
    ></textarea>
  </label>
  {#if replaced !== null}
    <p class="hint" data-testid="git-draft-replaced">
      An agent replaced your message.
      <button type="button" class="link" onclick={undoDraft}>Undo</button>
    </p>
  {/if}
  <button
    type="button"
    class="primary"
    data-testid="git-commit"
    aria-disabled={!!why || model.busy}
    aria-describedby={why && tried ? "git-commit-why" : undefined}
    aria-keyshortcuts={isApple() ? "Meta+Enter" : "Control+Enter"}
    onclick={commit}>{label}<kbd aria-hidden="true">{shortcut}</kbd></button
  >
  <!-- Each message shows once, here, and is read out from here. -->
  <div class="said" role="status">
    {#if why && tried}
      <p class="hint" id="git-commit-why">{repeating ? "" : why}</p>
    {/if}
    {#if model.committed && model.error?.scope !== "commit"}
      <p class="ok" data-testid="git-committed">
        Committed {model.committed.commit.id.slice(0, 7)}: {model.committed.commit.summary}
      </p>
    {/if}
  </div>
  {#if !status.head && !model.committed}
    <p class="hint" data-testid="git-first-commit">A commit saves a version of your files that you can go back to.</p>
  {/if}
  {#if model.error?.scope === "commit"}
    <div class="error" role="alert" data-testid="git-commit-error">
      <p>{model.error.message}</p>
      {#if model.error.output}
        <pre>{model.error.output}</pre>
      {/if}
    </div>
  {:else if model.committed}
    {#if model.committed.output.trim()}
      <details>
        <summary>Output of git and hooks</summary>
        <pre>{model.committed.output}</pre>
      </details>
    {/if}
  {/if}
</div>

<style>
  .commit {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .message {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    min-height: 3.5em;
    padding: 8px 10px;
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    resize: vertical;
  }
  textarea:focus-visible,
  button:focus-visible,
  summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .primary {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: var(--control-h);
    padding: 0 12px;
    font: inherit;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  kbd {
    font: inherit;
    font-size: 11px;
    font-weight: 400;
    opacity: 0.7;
  }
  .primary[aria-disabled="true"] {
    opacity: 0.5;
    cursor: default;
  }
  .hint,
  .ok,
  .error p {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .ok {
    color: var(--ok);
  }
  .error p {
    color: var(--danger);
  }
  pre {
    max-height: 12em;
    margin: 2px 0 0;
    padding: 6px 8px;
    overflow: auto;
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--s3);
    border-radius: var(--radius-sm);
  }
  .link {
    padding: 0;
    font: inherit;
    color: var(--accent);
    background: none;
    border: none;
    cursor: default;
  }
  .link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  details {
    font-size: 13px;
    color: var(--text-2);
  }
</style>
