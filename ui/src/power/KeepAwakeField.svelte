<script lang="ts">
// Settings > Claude: when Ovra keeps the computer awake. Every start of Ovra
// begins with "Off"; the core keeps the choice for this run.
import { untrack } from "svelte";
import type { KeepAwakeMode } from "../lib/protocol";
import { useOvra } from "../shell/context";
import { describe, KeepAwake } from "./keep-awake.svelte";

const { transport } = useOvra();
const power = new KeepAwake(transport);

const MODES: { mode: KeepAwakeMode; label: string }[] = [
  { mode: "off", label: "Off" },
  { mode: "while_agents_work", label: "While agents work" },
  { mode: "always", label: "Always, until Ovra quits" },
];

$effect(() => untrack(() => power.follow()));
</script>

{#if power.state}
  <fieldset class="awake" data-testid="keep-awake-field">
    <legend>Keep the computer awake</legend>
    {#each MODES as { mode, label } (mode)}
      <label>
        <input
          type="radio"
          name="keep-awake"
          value={mode}
          checked={power.state.mode === mode}
          onchange={() => void power.set(mode)}
        />
        {label}
      </label>
    {/each}
    <p class="hint" data-testid="keep-awake-state">{describe(power.state)}.</p>
    <p class="hint">
      Ovra stops only idle sleep: the display may turn off, and you can still put the computer to sleep. A closed laptop
      lid may still put it to sleep. Each time Ovra starts, this is Off.
    </p>
    {#if power.error}<p class="error" role="alert">{power.error}</p>{/if}
  </fieldset>
{/if}

<style>
  .awake {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 480px;
    margin: var(--sp-3) 0 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: var(--sp-1);
    padding: 0;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  p {
    margin: 0;
  }
  .hint {
    font: var(--t-small);
    color: var(--text-2);
  }
  .error {
    color: var(--danger);
  }
</style>
