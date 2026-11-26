import { flushSync, mount, unmount } from "svelte";
import { expect, it } from "vitest";
import Icon from "./Icon.svelte";
import { ICONS, type IconName } from "./icons";

/** The markup an icon's shapes serialize to once parsed. */
function drawn(icon: IconName): string {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.innerHTML = ICONS[icon];
  return svg.innerHTML;
}

it("draws each icon's shapes, as many times as it is shown, and follows a new name", () => {
  const target = document.createElement("div");
  let name = $state<IconName>("search");
  const props = {
    get name() {
      return name;
    },
  };
  const first = mount(Icon, { target, props });
  const second = mount(Icon, { target, props: { name: "search" } });
  flushSync();
  const [a, b] = [...target.querySelectorAll("svg")] as [SVGSVGElement, SVGSVGElement];
  expect(a.innerHTML).toBe(drawn("search"));
  expect(b.innerHTML).toBe(drawn("search"));
  expect(a.firstChild).not.toBe(b.firstChild);
  expect(a.firstElementChild?.namespaceURI).toBe("http://www.w3.org/2000/svg");
  name = "files";
  flushSync();
  expect(a.innerHTML).toBe(drawn("files"));
  expect(b.innerHTML).toBe(drawn("search"));
  unmount(first);
  unmount(second);
});
