<script lang="ts">
// "Explain these changes": on a press, a read-only agent says in plain
// words what the review's changes do and why. A line with Stop while it
// reads, then a short card; each file's line also shows above its diff.
// The agent's words are plain text, and they decide nothing.
import type { ReviewItem } from "../lib/protocol";
import Icon from "../shell/Icon.svelte";
import { fileName } from "./describe";
import type { Explainer } from "./explain.svelte";

let {
  explainer,
  item,
  task,
  onselect,
}: { explainer: Explainer; item: ReviewItem; task: string; onselect: (path: string) => void } = $props();

const current = $derived(explainer.state(item.id));
const stale = $derived(explainer.stale(item));
/** The files the agent explained, in the review's order. */
const explained = $derived(
  current?.kind === "done" ? item.files.filter((f) => f.path in current.explanation.files) : [],
);
</script>

{#if !current}
  <button
    type="button"
    class="quiet ask"
    data-testid="inbox-explain"
    title="An agent reads the changes and explains them in plain words. It changes nothing."
    aria-disabled={item.files.length === 0}
    onclick={() => void explainer.explain(item, task)}><Icon name="sparkles" small />Explain these changes</button
  >
{:else if current.kind === "starting" || current.kind === "running"}
  <div class="line" data-testid="inbox-explain-running">
    <p role="status">Reading the changes…</p>
    <button type="button" class="quiet" onclick={() => explainer.stop(item.id)}>Stop</button>
  </div>
{:else if current.kind === "error"}
  <div class="line error" data-testid="inbox-explain-error">
    <p role="alert">{current.message}</p>
    <span class="buttons">
      <button type="button" class="quiet" onclick={() => void explainer.explain(item, task)}>Try again</button>
      <button type="button" class="quiet" onclick={() => explainer.dismiss(item.id)}>Dismiss</button>
    </span>
  </div>
{:else}
  <section class="card" aria-labelledby="inbox-explain-title" data-testid="inbox-explanation">
    <h4 id="inbox-explain-title">In plain words</h4>
    {#each current.explanation.overall as line, i (i)}
      <p>{line}</p>
    {/each}
    {#if explained.length > 0}
      <ul aria-label="What changed in each file">
        {#each explained as f (f.path)}
          <li>
            <button type="button" class="link" title={f.path} onclick={() => onselect(f.path)}>{fileName(f)}</button>:
            {current.explanation.files[f.path]}
          </li>
        {/each}
      </ul>
    {/if}
    {#if stale}
      <p class="hint warn" data-testid="inbox-explain-stale">The changes moved on since this was written.</p>
    {/if}
    <p class="hint">Written by an agent from the diff: check the diff before you decide.</p>
    <div class="buttons">
      <button type="button" class="quiet" onclick={() => void explainer.explain(item, task)}
        >{stale ? "Explain again" : "Ask again"}</button
      >
      <button type="button" class="quiet" onclick={() => explainer.dismiss(item.id)}>Hide</button>
    </div>
  </section>
{/if}

<style>
  .ask {
    align-self: flex-start;
  }
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
    user-select: text;
  }
  h4 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding-left: 18px;
  }
  .hint {
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
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: inherit;
    border: 0;
    border-radius: var(--r-control);
    cursor: default;
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  button.quiet {
    padding: 3px 10px;
    color: var(--text-2);
    background: transparent;
  }
  button.quiet:hover {
    background: var(--s3);
  }
  button.link {
    display: inline;
    padding: 0;
    font-weight: 600;
    color: var(--text);
    background: transparent;
    text-decoration: underline;
    text-decoration-color: var(--separator, currentColor);
  }
  button.link:hover {
    background: transparent;
    text-decoration-color: currentColor;
  }
</style>
