<script lang="ts">
// The commit box: a message, "Amend last commit" and Commit. Mod+Enter in
// the message commits. Commit says why it waits; a hook's output is shown
// when the commit fails, and a short line names the commit when it is made.
import { formatShortcut, isApple } from "../keys/shortcut";
import type { RepoStatus } from "../lib/protocol";
import type { GitModel } from "./model.svelte";

let { model, status }: { model: GitModel; status: RepoStatus } = $props();

const shortcut = formatShortcut("Mod+Enter", isApple());
let text = $state("");
let amend = $state(false);

const staged = $derived(status.files.filter((f) => f.staged !== null && f.unstaged !== "conflicted").length);
const why = $derived.by(() => {
  if (!amend && staged === 0) return "Stage changes to commit them, or amend the last commit.";
  if (!text.trim()) return "Write a commit message.";
  return null;
});

/** The last commit's summary while it fills the box for an amend; cleared once the box is not that. */
let prefilled = $state<string | null>(null);

async function commit(): Promise<void> {
  if (model.busy) return;
  if (why) {
    // Mod+Enter from the message: say why nothing happened.
    model.notice = why;
    return;
  }
  if (await model.commit(text.trim(), amend)) {
    text = "";
    amend = false;
    prefilled = null;
  }
}

/** Amend starts from the last message (its first line: the protocol has no body); unticking takes it back out. */
async function onamend(): Promise<void> {
  if (!amend) {
    if (prefilled !== null && text === prefilled) text = "";
    prefilled = null;
    return;
  }
  if (text.trim()) return;
  const summary = await model.lastSummary();
  if (amend && !text.trim() && summary) {
    text = summary;
    prefilled = summary;
  }
}

function onkeydown(event: KeyboardEvent): void {
  if (event.code !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
  event.preventDefault();
  if (!event.repeat) void commit();
}
</script>

<div class="commit" role="group" aria-label="Commit">
  <label class="message">
    <span>Commit message</span>
    <textarea
      rows="3"
      spellcheck="true"
      placeholder={`${shortcut} commits`}
      aria-keyshortcuts={isApple() ? "Meta+Enter" : "Control+Enter"}
      bind:value={text}
      {onkeydown}
    ></textarea>
  </label>
  {#if status.head}
    <label class="check">
      <input type="checkbox" bind:checked={amend} onchange={onamend} />
      Amend last commit
    </label>
    {#if amend}
      <p class="hint">
        Replaces the last commit with what is staged and this message.{prefilled !== null && text === prefilled
          ? " The message is the last commit's first line; if it had more lines, add them again."
          : ""}
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
  {#if why}
    <p class="hint" id="git-commit-why">{why}</p>
  {/if}
  {#if model.error?.scope === "commit"}
    <div class="error" role="alert" data-testid="git-commit-error">
      <p>{model.error.message}</p>
      {#if model.error.output}
        <pre>{model.error.output}</pre>
      {/if}
    </div>
  {:else if model.committed}
    <p class="ok" data-testid="git-committed">
      Committed {model.committed.commit.id.slice(0, 7)}: {model.committed.commit.summary}
    </p>
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
    gap: 2px;
    font-size: 12px;
    color: #9a9aa6;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    min-height: 3.5em;
    padding: 4px 6px;
    font: inherit;
    color: #e6e6e6;
    background: #1e1e24;
    border: 1px solid #3a3a44;
    border-radius: 3px;
    resize: vertical;
  }
  textarea:focus-visible,
  input:focus-visible,
  button:focus-visible,
  summary:focus-visible {
    outline: 2px solid #4a6bb0;
    outline-offset: 1px;
  }
  .check {
    display: flex;
    gap: 4px;
    align-items: center;
    font-size: 12px;
  }
  .primary {
    padding: 3px 8px;
    font: inherit;
    color: #e6e6e6;
    background: #2f3d5c;
    border: 1px solid #4a6bb0;
    border-radius: 3px;
    cursor: pointer;
  }
  .primary[aria-disabled="true"] {
    opacity: 0.6;
    cursor: default;
  }
  .hint,
  .ok,
  .error p {
    margin: 0;
    font-size: 12px;
    color: #9a9aa6;
    overflow-wrap: anywhere;
  }
  .ok {
    color: #8fd19e;
  }
  .error p {
    color: #ff6b6b;
  }
  pre {
    max-height: 12em;
    margin: 2px 0 0;
    padding: 4px;
    overflow: auto;
    font-size: 11px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: #1e1e24;
    border-radius: 3px;
  }
  details {
    font-size: 12px;
  }
</style>
