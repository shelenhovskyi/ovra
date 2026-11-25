<script lang="ts">
// One of Ovra's icons (icons.ts), decorative: the button around it carries the name.
import { ICONS, type IconName } from "./icons";

let { name, small = false }: { name: IconName; small?: boolean } = $props();
</script>

<svg class="icon" class:small viewBox="0 0 24 24" aria-hidden="true" focusable="false">{@html ICONS[name]}</svg>

<style>
  .icon {
    width: 18px;
    height: 18px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .small {
    width: 15px;
    height: 15px;
  }
</style>
