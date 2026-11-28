<script lang="ts">
// What comes after Keep, on a kept item: take the keep back in one press
// while that is still possible, open a pull request of a worker's kept
// changes, and an agent that writes tests for what was kept. Each is a
// person's press; nothing here starts on its own.

import { reveal } from "../agents/reveal.svelte";
import { agentSessions } from "../agents/sessions.svelte";
import type { ReviewItem } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { mainView } from "../shell/main-view.svelte";
import { testsPrompt } from "./after-keep";
import { canOpenKeptPr } from "./pull-request";
import { message as errorText, type ReviewInbox } from "./reviews.svelte";

let {
  inbox,
  item,
  task,
  onundo,
  onpr,
}: { inbox: ReviewInbox; item: ReviewItem; task: string; onundo: () => void; onpr: () => void } = $props();

const { transport } = useOvra();
const sessions = agentSessions(transport);
const busy = $derived(inbox.busy.includes(item.id));
let starting = $state(false);
let error = $state<string | null>(null);
const files = $derived(item.files.filter((f) => f.kind !== "deleted").map((f) => f.path));

const undoHint = $derived(
  item.mode === "isolated"
    ? "Puts your files back as they were before the keep, and the review waits again."
    : "The review waits again, so you can still discard the agent's changes. Your files stay as they are.",
);

async function writeTests(): Promise<void> {
  if (starting || files.length === 0) return;
  starting = true;
  error = null;
  try {
    const started = await sessions.start(item.project, testsPrompt(task, files));
    reveal.session = started.id;
    mainView.close(false);
  } catch (err) {
    error = `Could not start an agent: ${errorText(err)}`;
  } finally {
    starting = false;
  }
}
</script>

<div class="actions" role="group" aria-label="After the keep">
  {#if item.can_undo_keep}
    <p class="hint">{undoHint}</p>
  {/if}
  <div class="buttons">
    {#if item.can_undo_keep}
      <button
        type="button"
        data-testid="inbox-undo-keep"
        aria-disabled={busy}
        onclick={() => {
          if (!busy) onundo();
        }}>Undo keep</button
      >
    {/if}
    {#if canOpenKeptPr(item)}
      <button
        type="button"
        class="quiet"
        data-open-pr
        title="Pushes the worker's kept changes to a new branch and opens a pull request with your gh."
        aria-disabled={busy}
        onclick={() => {
          if (!busy) onpr();
        }}>Open pull request…</button
      >
    {/if}
    {#if files.length > 0}
      <button
        type="button"
        class="quiet"
        data-testid="inbox-write-tests"
        title="Starts an agent in the project that writes and runs tests for the kept files."
        aria-disabled={starting}
        onclick={() => void writeTests()}>Write tests</button
      >
    {/if}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 20px 0;
  }
  p {
    margin: 0;
  }
  .hint {
    color: var(--text-2);
    font-size: var(--fs-ui);
  }
  .error {
    color: var(--danger);
    font-size: var(--fs-ui);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  /* A component of its own: the inbox's button styles do not reach it. */
  button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: inherit;
    font-weight: 500;
    padding: 4px 11px;
    color: var(--text);
    background: var(--s3);
    border: 1px solid transparent;
    border-radius: var(--r-control);
  }
  button:not([aria-disabled="true"]):hover {
    background: var(--s4);
  }
  button[aria-disabled="true"] {
    opacity: 0.55;
  }
  .quiet {
    color: var(--text-2);
    background: transparent;
  }
  .quiet:not([aria-disabled="true"]):hover {
    background: var(--s3);
  }
</style>
