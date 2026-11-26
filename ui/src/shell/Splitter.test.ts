import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import Splitter from "./Splitter.svelte";

const apps: ReturnType<typeof mount>[] = [];
afterEach(() => {
  for (const app of apps.splice(0)) unmount(app);
  document.body.innerHTML = "";
});

function setup() {
  const sizes: number[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  apps.push(
    mount(Splitter, {
      target,
      props: {
        label: "Resize",
        width: 200,
        max: 600,
        edge: "bottom",
        onresize: (h) => sizes.push(h),
        onreset: () => {},
      },
    }),
  );
  const handle = target.querySelector<HTMLElement>("[role=separator]");
  if (!handle) throw new Error("no handle");
  const pointer = (el: Element, type: string, clientY: number, buttons: number) => {
    // jsdom has no PointerEvent; a MouseEvent carries the fields the handle reads.
    el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, buttons, clientY }));
    flushSync();
  };
  const overlay = () => target.querySelector(".dragging");
  return { handle, sizes, pointer, overlay };
}

it("resizes while dragging and ends on pointerup", () => {
  const { handle, sizes, pointer, overlay } = setup();
  pointer(handle, "pointerdown", 300, 1);
  expect(overlay()).not.toBeNull();
  pointer(handle, "pointermove", 280, 1);
  expect(sizes).toEqual([220]);
  pointer(handle, "pointerup", 280, 0);
  expect(overlay()).toBeNull();
});

it("ends the drag when capture is lost, so the overlay does not stay", () => {
  const { handle, pointer, overlay } = setup();
  pointer(handle, "pointerdown", 300, 1);
  handle.dispatchEvent(new Event("lostpointercapture"));
  flushSync();
  expect(overlay()).toBeNull();
});

it("ends a drag whose button came up unseen on the next move", () => {
  const { handle, sizes, pointer, overlay } = setup();
  pointer(handle, "pointerdown", 300, 1);
  const shown = overlay();
  if (!shown) throw new Error("no overlay");
  pointer(shown, "pointermove", 250, 0);
  expect(overlay()).toBeNull();
  expect(sizes).toEqual([]);
});
