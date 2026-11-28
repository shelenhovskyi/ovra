import { flushSync, mount, unmount } from "svelte";
import { expect, it } from "vitest";
import { mainView } from "./main-view.svelte";
import ViewFrameHarness from "./ViewFrameHarness.svelte";

it("leaves a filled field on the first Escape, keeping its text, and closes on the next", () => {
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(ViewFrameHarness, { target });
  mainView.show("audit");
  flushSync();
  const field = target.querySelector<HTMLInputElement>("input[aria-label=Match]");
  const pressEscape = (el: Element | null | undefined) =>
    el?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true }));
  if (!field) throw new Error("no field");
  field.value = "npm test";
  field.focus();
  pressEscape(field);
  flushSync();
  expect(mainView.current).toBe("audit");
  expect(field.value).toBe("npm test");
  expect(document.activeElement?.textContent).toBe("Audit log");
  pressEscape(document.activeElement);
  flushSync();
  expect(mainView.current).toBeNull();

  // An empty field, or a checkbox, does not hold the view open.
  for (const selector of ["input[aria-label=Match]", "input[aria-label=Tick]"]) {
    mainView.show("audit");
    flushSync();
    const control = target.querySelector<HTMLInputElement>(selector);
    if (control?.type === "text") control.value = "";
    control?.focus();
    pressEscape(control);
    flushSync();
    expect(mainView.current).toBeNull();
  }
  unmount(app);
  target.remove();
});
