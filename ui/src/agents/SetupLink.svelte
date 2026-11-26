<script lang="ts">
// The empty agents screen's way to Settings > Setup check: a quiet link,
// or a warning line naming how many problems the check found, so a person
// whose first task would fail learns why before starting it.
import { untrack } from "svelte";
import { openSettings } from "../settings/commands";
import { checkRows } from "../settings/setup-check";
import { useOvra } from "../shell/context";

const { transport, workspace } = useOvra();

let problems = $state(0);

$effect(() => {
  const project = workspace.project?.id ?? null;
  untrack(() => {
    transport
      .call({ method: "setup.check", params: { project } })
      .then(({ data }) => (problems = checkRows(data).filter((row) => row.level === "problem").length))
      // A check that could not run says nothing here; the pane says why.
      .catch(() => (problems = 0));
  });
});
</script>

{#if problems > 0}
  <p class="problems" role="note" data-testid="agents-setup-problems">
    Setup check found {problems === 1 ? "a problem" : `${problems} problems`} on this computer.
    <button type="button" onclick={() => openSettings("setup")}>Check setup</button>
  </p>
{:else}
  <p class="quiet"><button type="button" class="link" onclick={() => openSettings("setup")}>Check setup</button></p>
{/if}

<style>
  p {
    margin: 0 12px 8px;
    font: var(--t-small);
  }
  .problems {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    padding: 8px 10px;
    margin: 0 8px 8px;
    color: var(--text);
    background: var(--warn-soft);
    border-radius: var(--radius);
  }
  button {
    height: var(--ctl-sm);
    padding: 0 var(--sp-2);
    font: inherit;
    color: var(--text);
    background: var(--s3);
    border: 0;
    border-radius: var(--r-md);
  }
  button:hover {
    background: var(--s4);
  }
  button.link {
    height: auto;
    padding: 0;
    color: var(--text-2);
    background: transparent;
  }
  button.link:hover {
    color: var(--text);
    text-decoration: underline;
    background: transparent;
  }
  @media (forced-colors: active) {
    .problems {
      border: 1px solid CanvasText;
    }
  }
</style>
