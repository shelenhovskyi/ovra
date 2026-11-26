<script lang="ts">
// A task's outcome in three short lines, before its changes: what the agent
// did, how it checked it, and what is left. Shown only when the agent wrote
// them (see summary.ts); a line it left out says so instead of guessing.
import { nothingLeft, type TaskSummary } from "./summary";

let { summary }: { summary: TaskSummary } = $props();

const rows = $derived([
  { key: "done", label: "Done", text: summary.done, unsaid: false, calm: false },
  {
    key: "checked",
    label: "Checked",
    text: summary.checked ?? "Not said",
    unsaid: summary.checked === null,
    calm: false,
  },
  {
    key: "left",
    label: "Left",
    text: summary.left ?? "Not said",
    unsaid: summary.left === null,
    calm: nothingLeft(summary.left),
  },
]);
</script>

<dl class="task-summary" aria-label="Task summary" data-testid="task-summary">
  {#each rows as row (row.key)}
    <div class="row" data-line={row.key} class:unsaid={row.unsaid} class:calm={row.calm}>
      <dt>{row.label}</dt>
      <dd>{row.text}</dd>
    </div>
  {/each}
</dl>

<style>
  .task-summary {
    margin: 0;
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    font: var(--t-small);
  }
  .row {
    display: contents;
  }
  dt {
    color: var(--text-2);
  }
  dd {
    margin: 0;
    min-width: 0;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .unsaid dd,
  .calm dd {
    color: var(--text-2);
  }
</style>
