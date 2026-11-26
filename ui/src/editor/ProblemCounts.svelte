<script lang="ts">
// The open project's problem counts in the status bar ("2 errors, 1
// warning"), while there are any; a click shows the Problems list.
import { listCommands } from "../keys/commands";
import { useOvra } from "../shell/context";
import { countProblems, countsLabel, problems } from "./lsp/problems.svelte";

const { workspace } = useOvra();

const label = $derived(countsLabel(countProblems(problems.files(workspace.project?.id))));

function showProblems(): void {
  void listCommands()
    .find((command) => command.id === "problems.toggle")
    ?.run();
}
</script>

{#if label}
  <button type="button" title="Show problems" data-testid="problem-counts" onclick={showProblems}>{label}</button>
{/if}

<style>
  button {
    padding: 0 6px;
    font: inherit;
    color: inherit;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
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
