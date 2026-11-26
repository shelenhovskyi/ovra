<script lang="ts" module>
import { ICONS, type IconName } from "./icons";

// Each icon's markup is parsed once and cloned after that: a screen shows dozens
// of icons, and parsing markup is far slower than cloning nodes.
const parsed = new Map<IconName, DocumentFragment>();

function shapes(icon: IconName) {
  return (svg: SVGSVGElement) => {
    let shape = parsed.get(icon);
    if (!shape) {
      const holder = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      holder.innerHTML = ICONS[icon];
      shape = document.createDocumentFragment();
      shape.append(...holder.childNodes);
      parsed.set(icon, shape);
    }
    svg.replaceChildren(shape.cloneNode(true));
  };
}
</script>

<script lang="ts">
// One of Ovra's icons (icons.ts), decorative: the button around it carries the name.
// 18 px alone (rail, title bar), 16 px (`small`) beside text.

let { name, small = false }: { name: IconName; small?: boolean } = $props();
</script>

<svg class="icon" class:small viewBox="0 0 24 24" aria-hidden="true" focusable="false" {@attach shapes(name)}></svg>

<style>
  .icon {
    width: 18px;
    height: 18px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.75;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .small {
    width: 16px;
    height: 16px;
  }
</style>
