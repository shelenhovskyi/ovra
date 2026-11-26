<script lang="ts">
// Asks about a drop from outside: names that exist already (Replace, Keep
// both, Cancel) or names that look like secrets (Copy anyway, Cancel). The
// safe answer has focus, so Enter never replaces or copies a secret; Tab
// cycles the buttons, and Escape or focus going elsewhere cancels.
import type { DropAnswer } from "./drop-import.svelte";

let {
  kind,
  question,
  onanswer,
}: {
  kind: "clash" | "secret";
  question: string;
  /** null: cancel. */
  onanswer: (answer: DropAnswer) => void;
} = $props();

let card: HTMLDivElement | undefined = $state();
let safe: HTMLButtonElement | undefined = $state();

$effect(() => {
  safe?.focus();
});

function onkeydown(e: KeyboardEvent): void {
  if (e.code === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    onanswer(null);
  } else if (e.code === "Tab" && card) {
    e.preventDefault();
    const buttons = [...card.querySelectorAll("button")];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = (at + (e.shiftKey ? buttons.length - 1 : 1)) % buttons.length;
    buttons[next]?.focus();
  }
}

function onfocusout(e: FocusEvent): void {
  if (e.relatedTarget instanceof Node && card?.contains(e.relatedTarget)) return;
  onanswer(null);
}
</script>

<div
  class="confirm"
  role="alertdialog"
  aria-label={kind === "clash" ? "Replace files" : "Copy files that may hold secrets"}
  tabindex="-1"
  bind:this={card}
  {onkeydown}
  {onfocusout}
>
  <p>{question}</p>
  <div class="buttons">
    {#if kind === "clash"}
      <button type="button" class="danger" onclick={() => onanswer("replace")}>Replace</button>
      <button type="button" class="plain" bind:this={safe} onclick={() => onanswer("keep_both")}>Keep both</button>
      <button type="button" class="quiet" onclick={() => onanswer(null)}>Cancel</button>
    {:else}
      <button type="button" class="danger" onclick={() => onanswer("copy")}>Copy anyway</button>
      <button type="button" class="plain" bind:this={safe} onclick={() => onanswer(null)}>Cancel</button>
    {/if}
  </div>
</div>

<style>
  .confirm {
    position: absolute;
    z-index: 2;
    top: 40px;
    left: 8px;
    right: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--s4);
    box-shadow: var(--card-shadow);
  }
  p {
    margin: 0 0 8px;
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  button {
    min-height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    font: inherit;
  }
  .danger {
    background: var(--danger-soft);
    color: var(--danger);
    font-weight: 500;
  }
  .plain {
    background: var(--s3);
    color: var(--text);
  }
  .quiet {
    background: transparent;
    color: var(--text-2);
  }
  .quiet:hover {
    background: var(--s3);
    color: var(--text);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
