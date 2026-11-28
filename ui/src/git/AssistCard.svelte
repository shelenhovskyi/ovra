<script lang="ts">
// What an agent action in the git panel has to say: a line with Stop while
// it works, the suggested commits with Commit all, a short text answer, or
// what went wrong. The agent's words are shown as plain text.
import { tick } from "svelte";
import { type Assist, RUNNING, TEXT_TITLE } from "./assist.svelte";
import { describePaths } from "./model.svelte";

let { assist }: { assist: Assist } = $props();

const current = $derived(assist.state);
let card = $state<HTMLElement>();

/** First line of a message, and the rest. */
function split(text: string): [string, string] {
  const [first = "", ...rest] = text.split("\n");
  return [first, rest.join("\n").trim()];
}

async function commitAll(): Promise<void> {
  await assist.commitAll();
  await tick();
  // Done: the card is gone; focus goes back to the panel's first control.
  if (!card?.isConnected) document.querySelector<HTMLElement>("[data-testid=git-assist]")?.focus();
}

/** Back to the panel's agent actions button, so focus never falls to the page. */
async function backToButton(): Promise<void> {
  await tick();
  document.querySelector<HTMLElement>("[data-testid=git-assist]")?.focus();
}

function dismiss(): void {
  if (current.kind === "plan" && current.committing) return;
  assist.dismiss();
  void backToButton();
}

/** Escape dismisses the card; Mod+Enter in a suggested message commits them all. */
function onkeydown(event: KeyboardEvent): void {
  if (event.code === "Escape" && current.kind !== "starting" && current.kind !== "running") {
    event.preventDefault();
    dismiss();
  } else if (event.code === "Enter" && (event.ctrlKey || event.metaKey) && current.kind === "plan") {
    event.preventDefault();
    if (!event.repeat) void commitAll();
  }
}

$effect(() => {
  // A new result takes focus once, so keyboard and screen reader users find it;
  // edits to the plan do not move focus.
  if (assist.result === 0) return;
  void tick().then(() => card?.querySelector<HTMLElement>("[data-first]")?.focus());
});
</script>

{#if current.kind === "starting" || current.kind === "running"}
  <div class="line" data-testid="git-assist-running" bind:this={card}>
    <p role="status">{RUNNING[current.action]}</p>
    <button type="button" class="quiet" onclick={() => assist.stop()}>Stop</button>
  </div>
{:else if current.kind === "error"}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="line error" role="group" aria-label="Agent action failed" data-testid="git-assist-error" bind:this={card} {onkeydown}>
    <p role="alert">{current.message}</p>
    <span class="buttons">
      <button type="button" class="quiet" data-first onclick={() => void assist.run(current.action)}>Try again</button>
      <button type="button" class="quiet" onclick={dismiss}>Dismiss</button>
    </span>
  </div>
{:else if current.kind === "text"}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section class="card" aria-label={TEXT_TITLE[current.action]} data-testid="git-assist-text" bind:this={card} {onkeydown}>
    <h3 tabindex="-1" data-first>{TEXT_TITLE[current.action]}</h3>
    <ul>
      {#each current.lines as line, i (i)}
        <li>{line}</li>
      {/each}
    </ul>
    <div class="buttons">
      <button type="button" class="quiet" onclick={dismiss}>Dismiss</button>
    </div>
  </section>
{:else if current.kind === "plan"}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section class="card" aria-label="Suggested commits" data-testid="git-assist-plan" bind:this={card} {onkeydown}>
    <h3 tabindex="-1" data-first>
      {current.commits.length === 1 ? "1 suggested commit" : `${current.commits.length} suggested commits`}
    </h3>
    <ol>
      {#each current.commits as commit, i (i)}
        {@const [subject, body] = split(commit.message)}
        <li>
          <label class="message">
            <span class="visually-hidden">Message of commit {i + 1}</span>
            <textarea
              rows={body ? 3 : 1}
              spellcheck="true"
              aria-disabled={current.committing}
              readonly={current.committing}
              value={commit.message}
              oninput={(e) => assist.edit(i, e.currentTarget.value)}
            ></textarea>
          </label>
          <p class="files" title={commit.files.join("\n")}>
            {subject ? "" : "No message. "}{describePaths(commit.files)}{commit.files.length > 1
              ? `: ${commit.files.slice(0, 3).join(", ")}${commit.files.length > 3 ? ", …" : ""}`
              : ""}
          </p>
          {#if !current.committing}
            <button
              type="button"
              class="quiet small"
              aria-label={`Skip commit ${i + 1}`}
              onclick={() => {
                assist.drop(i);
                if (assist.state.kind !== "plan") void backToButton();
              }}>Skip this commit</button
            >
          {/if}
        </li>
      {/each}
    </ol>
    {#if current.left.length}
      <p class="hint" title={current.left.join("\n")}>
        {current.left.length <= 3 ? current.left.join(", ") : describePaths(current.left)}
        {current.left.length === 1 ? "stays" : "stay"} uncommitted.
      </p>
    {/if}
    {#if current.partly.length}
      <p class="hint">
        {describePaths(current.partly)}
        {current.partly.length === 1 ? "is" : "are"} staged in part; Commit all commits whole files.
      </p>
    {/if}
    {#if current.confirm}
      <p class="hint warn" role="alert" data-testid="git-assist-stale">
        Your changes moved on since this was suggested. Press Commit anyway to commit the files as they are now.
      </p>
    {:else if assist.stale}
      <p class="hint warn" data-testid="git-assist-stale">Your changes moved on since this was suggested.</p>
    {/if}
    <div class="buttons">
      <button
        type="button"
        class="primary"
        data-testid="git-assist-commit-all"
        aria-disabled={current.committing || current.commits.some((c) => !c.message.trim())}
        onclick={commitAll}
        >{current.committing
          ? "Committing…"
          : current.confirm
            ? "Commit anyway"
            : current.commits.length === 1
              ? "Commit"
              : "Commit all"}</button
      >
      <button type="button" class="quiet" aria-disabled={current.committing} onclick={dismiss}>Dismiss</button>
    </div>
  </section>
{/if}

<style>
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    font-size: var(--fs-ui);
    color: var(--text-2);
  }
  .line p {
    flex: 1 1 12em;
    min-width: 0;
    margin: 0;
  }
  .error {
    flex-direction: column;
    align-items: flex-start;
  }
  .error p {
    flex: none;
    color: var(--danger);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: var(--s2);
    border-radius: var(--r-lg, var(--radius));
    font-size: var(--fs-ui);
  }
  h3 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  h3:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  ul,
  ol {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding-left: 18px;
    user-select: text;
  }
  ol li {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-family: var(--font-mono, monospace);
    font-size: var(--fs-ui-sm);
    color: var(--text);
    background: var(--s1);
    border: 1px solid var(--separator, transparent);
    border-radius: var(--radius-sm);
    padding: 4px 6px;
    resize: vertical;
  }
  .files {
    margin: 0;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    margin: 0;
    color: var(--text-2);
  }
  .warn {
    color: var(--warn);
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  button {
    font: inherit;
    border: 1px solid transparent;
    border-radius: var(--r-control);
    padding: 3px 10px;
    cursor: default;
  }
  .quiet {
    color: var(--text-2);
    background: transparent;
  }
  .quiet:hover {
    background: var(--s3);
  }
  .small {
    align-self: flex-start;
    padding: 0 6px;
    font-size: var(--fs-ui-sm);
  }
  .primary {
    color: var(--accent-ink, white);
    background: var(--accent);
    font-weight: 500;
  }
  button[aria-disabled="true"] {
    opacity: 0.6;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
