<script lang="ts">
// Asks before a file or folder goes to the trash. Move to Trash has focus, so
// Enter answers as the menu item asked; Escape cancels.
let { name, onconfirm, oncancel }: { name: string; onconfirm: () => void; oncancel: () => void } = $props();

let confirm: HTMLButtonElement | undefined = $state();

$effect(() => {
  confirm?.focus();
});

function onkeydown(e: KeyboardEvent): void {
  if (e.code !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  oncancel();
}
</script>

<div class="confirm" role="alertdialog" aria-label="Move to Trash" tabindex="-1" {onkeydown}>
  <p>Move “{name}” to Trash?</p>
  <div class="buttons">
    <button type="button" class="danger" bind:this={confirm} onclick={onconfirm}>Move to Trash</button>
    <button type="button" class="quiet" onclick={oncancel}>Cancel</button>
  </div>
</div>

<style>
  .confirm {
    margin: 0 8px 6px;
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
