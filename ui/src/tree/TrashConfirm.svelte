<script lang="ts">
// Asks before a file or folder goes to the trash, next to its row. Move to
// Trash has focus, so Enter answers as the menu item asked; Tab stays on the
// two buttons, and Escape or focus going elsewhere cancels.
let {
  question,
  warning = null,
  top = 0,
  oncard,
  onconfirm,
  oncancel,
}: {
  question: string;
  /** What else is lost, such as unsaved changes. */
  warning?: string | null;
  /** Where the card sits in the panel, in px. */
  top?: number;
  /** The card's height, for placing it above a row near the bottom. */
  oncard?: (height: number) => void;
  onconfirm: () => void;
  /** `refocus` is false when focus already went elsewhere. */
  oncancel: (refocus: boolean) => void;
} = $props();

let card: HTMLDivElement | undefined = $state();
let confirm: HTMLButtonElement | undefined = $state();
let cancel: HTMLButtonElement | undefined = $state();
let height = $state(0);

$effect(() => {
  confirm?.focus();
});

$effect(() => {
  oncard?.(height);
});

function onkeydown(e: KeyboardEvent): void {
  if (e.code === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    oncancel(true);
  } else if (e.code === "Tab") {
    e.preventDefault();
    (document.activeElement === confirm ? cancel : confirm)?.focus();
  }
}

function onfocusout(e: FocusEvent): void {
  if (e.relatedTarget instanceof Node && card?.contains(e.relatedTarget)) return;
  oncancel(false);
}
</script>

<div
  class="confirm"
  role="alertdialog"
  aria-label="Move to Trash"
  aria-describedby={warning ? "trash-warning" : undefined}
  tabindex="-1"
  style:top="{top}px"
  bind:this={card}
  bind:clientHeight={height}
  {onkeydown}
  {onfocusout}
>
  <p>{question}</p>
  {#if warning}<p id="trash-warning" class="warning">{warning}</p>{/if}
  <div class="buttons">
    <button type="button" class="danger" bind:this={confirm} onclick={onconfirm}>Move to Trash</button>
    <button type="button" class="quiet" bind:this={cancel} onclick={() => oncancel(true)}>Cancel</button>
  </div>
</div>

<style>
  .confirm {
    position: absolute;
    z-index: 2;
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
  .warning {
    color: var(--warn);
  }
  .buttons {
    display: flex;
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
