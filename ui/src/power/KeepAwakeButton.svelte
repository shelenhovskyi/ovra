<script lang="ts">
// The Agents panel's keep-awake switch: on keeps the computer from sleeping
// while agents work, off lets it sleep as usual. Hidden when the core has no
// power service. The mode "always" is in Settings > Claude.
import { untrack } from "svelte";
import { useOvra } from "../shell/context";
import Icon from "../shell/Icon.svelte";
import { describe, KeepAwake } from "./keep-awake.svelte";

const { transport } = useOvra();
const power = new KeepAwake(transport);

$effect(() => untrack(() => power.follow()));
</script>

{#if power.state}
  <button
    type="button"
    class="quiet icon-only keep-awake"
    class:awake={power.state.awake}
    class:problem={!!power.state.problem}
    data-testid="keep-awake"
    aria-label="Keep the computer awake while agents work"
    aria-pressed={power.state.mode !== "off"}
    title={power.error ?? describe(power.state)}
    onclick={() => void power.toggle()}><Icon name="awake" small /></button
  >
{/if}

<style>
  .keep-awake[aria-pressed="true"] {
    color: var(--text);
  }
  .keep-awake.awake {
    color: var(--accent);
  }
  .keep-awake.problem {
    color: var(--warn);
  }
</style>
