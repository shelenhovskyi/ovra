import { flushSync, mount, unmount } from "svelte";
import { expect, it } from "vitest";
import FirstSteps from "./FirstSteps.svelte";
import { hints } from "./hints.svelte";

it("ticks the first steps off as they are done, and hides for good", () => {
  const app = mount(FirstSteps, { target: document.body, props: { ready: true, started: false } });
  const card = () => document.querySelector("[data-testid=first-steps]");
  const words = () => card()?.textContent?.replace(/\s+/g, " ").trim();
  expect(words()).toMatch(/^Get started 1 of 3 Hide/);
  expect(card()?.querySelector("[aria-current=step]")?.textContent).toContain("Type a task and press Start");
  hints.mark("started");
  flushSync();
  expect(words()).toMatch(/^Get started 2 of 3/);
  card()?.querySelector("button")?.click();
  flushSync();
  expect(card()).toBeNull();
  expect(JSON.parse(localStorage.getItem("ovra.hints") ?? "{}")).toMatchObject({ started: true, steps_hidden: true });
  unmount(app);
});
