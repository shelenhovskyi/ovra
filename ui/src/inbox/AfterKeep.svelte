<script lang="ts">
// What comes after Keep, on a kept item: take the keep back in one press
// while that is still possible, and the usual next step, an agent that
// writes tests for what was kept. Each is a person's press; nothing here
// starts on its own.

import { reveal } from "../agents/reveal.svelte";
import { agentSessions } from "../agents/sessions.svelte";
import type { ReviewItem } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { mainView } from "../shell/main-view.svelte";
import { testsPrompt } from "./after-keep";
import { message as errorText, type ReviewInbox } from "./reviews.svelte";

let { inbox, item, task, onundo }: { inbox: ReviewInbox; item: ReviewItem; task: string; onundo: () => void } =
  $props();

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
    font-size: 13px;
  }
  .error {
    color: var(--danger);
    font-size: 13px;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .quiet {
    color: var(--text-2);
    background: transparent;
  }
  .quiet:hover {
    background: var(--s3);
  }
</style>
