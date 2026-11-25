<script lang="ts">
// The shown file's quiet facts in the status bar (status.svelte.ts): the
// cursor, which opens "Go to line…", then language, indentation and line endings.
import { listCommands } from "../keys/commands";
import { cursorLabel, fileStatus } from "./status.svelte";

const facts = $derived(fileStatus.facts);

function goToLine(): void {
  void listCommands()
    .find((command) => command.id === "editor.go_to_line")
    ?.run();
}
</script>

{#if facts}
  <span class="facts" data-testid="file-status">
    {#if facts.cursor}
      <button type="button" title="Go to line…" onclick={goToLine}>{cursorLabel(facts.cursor)}</button>
    {/if}
    <span>{facts.language}</span>
    <span>{facts.indent}</span>
    <span>{facts.lineEnding}</span>
  </span>
{/if}

<style>
  .facts {
    display: flex;
    align-items: center;
    gap: 14px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  button {
    padding: 0 6px;
    margin: 0 -6px;
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    border-radius: var(--r-sm);
    cursor: default;
  }
  button:hover {
    color: var(--text);
    background: var(--s3);
  }
</style>
