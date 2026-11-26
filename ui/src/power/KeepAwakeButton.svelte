<script lang="ts">
// The Agents panel's keep-awake switch: on keeps the computer from sleeping
// while agents work, off lets it sleep as usual. Hidden when the core has no
// power service. The mode "always" is in Settings > Claude. It styles itself:
// the panel's button classes are scoped to the panel and do not reach here.
// A cup alone says little, so hovering or focusing it says what it does at once.
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
    onclick={() => void power.toggle()}
    ><Icon name="awake" small /><span class="tip" role="tooltip" data-testid="keep-awake-tip"
      ><strong>Keep awake: {power.state.mode === "off" ? "off" : "on"}</strong>
      <span>{power.error ?? describe(power.state)}</span></span
    ></button
  >
{/if}

<style>
  .keep-awake {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--text-2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
  }
  .keep-awake:hover {
    color: var(--text);
    background: var(--s3);
  }
  .tip {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 50;
    width: max-content;
    max-width: 240px;
    padding: 6px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font: var(--t-small);
    text-align: left;
    white-space: normal;
    color: var(--text);
    background: var(--s4);
    border-radius: var(--radius-sm);
    box-shadow: var(--card-shadow);
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.12s;
  }
  .tip span {
    color: var(--text-2);
  }
  .keep-awake:hover .tip,
  .keep-awake:focus-visible .tip {
    opacity: 1;
    transition-delay: 0.3s;
  }
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
