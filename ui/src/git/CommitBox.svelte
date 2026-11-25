<script lang="ts">
// The commit box: a message, "Amend last commit" and Commit. Mod+Enter
// anywhere in the box commits. Commit says why it waits; a hook's output is shown
// when the commit fails, and a short line names the commit when it is made.
import { tick } from "svelte";
import { formatShortcut, isApple } from "../keys/shortcut";
import type { RepoStatus } from "../lib/protocol";
import type { GitModel } from "./model.svelte";

let { model, status }: { model: GitModel; status: RepoStatus } = $props();

const shortcut = formatShortcut("Mod+Enter", isApple());
let text = $state("");
let amend = $state(false);

const staged = $derived(status.files.filter((f) => f.staged !== null && f.unstaged !== "conflicted").length);
const why = $derived.by(() => {
  if (!amend && staged === 0) return status.head ? "Stage files, or amend." : "Stage files first.";
  if (!text.trim() && !amend) return "Write a commit message.";
  return null;
});

/** Blanks the reason for a moment, so that its live region reads it out again. */
let repeating = $state(false);

async function commit(): Promise<void> {
  if (model.busy) return;
  if (why) {
    // Mod+Enter from the message: say why nothing happened, where it shows.
    repeating = true;
    await tick();
    repeating = false;
    return;
  }
  if (await model.commit(text.trim(), amend)) {
    text = "";
    amend = false;
  }
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
  event.preventDefault();
  if (!event.repeat) void commit();
}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="commit" role="group" aria-label="Commit" {onkeydown}>
  <label class="message">
    <span>Commit message</span>
    <textarea
      rows="3"
      spellcheck="true"
      placeholder={`${shortcut} commits`}
      aria-keyshortcuts={isApple() ? "Meta+Enter" : "Control+Enter"}
      bind:value={text}
    ></textarea>
  </label>
  {#if status.head}
    <label class="check">
      <input type="checkbox" bind:checked={amend} />
      Amend last commit
    </label>
    {#if amend}
      <p class="hint">
        Replaces the last commit with what is staged. An empty message keeps the old one.
      </p>
    {/if}
  {/if}
  <button
    type="button"
    class="primary"
    data-testid="git-commit"
    aria-disabled={!!why || model.busy}
    aria-describedby={why ? "git-commit-why" : undefined}
    onclick={commit}>{amend ? "Amend commit" : "Commit"}</button
  >
  <!-- Each message shows once, here, and is read out from here. -->
  <div class="said" role="status">
    {#if why}
      <p class="hint" id="git-commit-why">{repeating ? "" : why}</p>
    {/if}
    {#if model.committed && model.error?.scope !== "commit"}
      <p class="ok" data-testid="git-committed">
        Committed {model.committed.commit.id.slice(0, 7)}: {model.committed.commit.summary}
      </p>
    {/if}
  </div>
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
  input:focus-visible,
  button:focus-visible,
  summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .check {
    display: flex;
    gap: 4px;
    align-items: center;
    font-size: 13px;
  }
  .primary {
    padding: 5px 12px;
    font: inherit;
    font-weight: 500;
    color: var(--accent-ink);
    background: var(--accent);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
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
  details {
    font-size: 13px;
    color: var(--text-2);
  }
</style>
