import { flushSync, mount, unmount } from "svelte";
import { expect, it } from "vitest";
import FirstSteps from "./FirstSteps.svelte";
import { hints } from "./hints.svelte";

it("says what to do for the last step, with a button when there is something to open", () => {
  const opened: string[] = [];
  const props = $state<{
    agent: string;
    ready: boolean;
    started: boolean;
    ask?: () => void;
    review?: () => void;
    shown?: boolean;
    asks?: number;
  }>({
    agent: "Claude",
    ready: true,
    started: true,
  });
  const app = mount(FirstSteps, { target: document.body, props });
  const detail = () => document.querySelector("[data-testid=first-steps-detail]")?.textContent;
  const button = () => document.querySelector<HTMLButtonElement>("[data-testid=first-steps] li[aria-current] button");
  expect(detail()).toBe("Claude is working. Its questions and changes show up here.");
  expect(button()).toBeNull();
  props.ask = () => opened.push("ask");
  flushSync();
  expect(detail()).toBe("Claude asks you something.");
  button()?.click();
  // The request is on screen already: no Open, and the count when several ask.
  Object.assign(props, { shown: true, asks: 2 });
  flushSync();
  expect(detail()).toBe("Answer the 2 requests above.");
  expect(button()).toBeNull();
  Object.assign(props, { shown: false, asks: 1 });
  props.ask = undefined;
  props.review = () => opened.push("review");
  flushSync();
  expect(detail()).toBe("Its changes are ready.");
  expect(button()?.textContent).toBe("Review");
  button()?.click();
  expect(opened).toEqual(["ask", "review"]);
  unmount(app);
});

it("ticks the first steps off as they are done, and hides for good", () => {
  const app = mount(FirstSteps, { target: document.body, props: { agent: "Claude", ready: true, started: false } });
  const card = () => document.querySelector("[data-testid=first-steps]");
  const words = () => card()?.textContent?.replace(/\s+/g, " ").trim();
  expect(words()).toMatch(/^Get started 1 of 3 Hide/);
  expect(card()?.querySelector("[aria-current=step]")?.textContent).toContain("Type a task and press Start");
  hints.mark("started");
  flushSync();
  expect(words()).toMatch(/^Get started 2 of 3/);
  expect(card()?.querySelectorAll("li")).toHaveLength(3);
  unmount(app);
  // With several tasks, only the step to do next shows.
  const busy = mount(FirstSteps, {
    target: document.body,
    props: { agent: "Claude", ready: true, started: true, compact: true },
  });
  expect([...(card()?.querySelectorAll("li") ?? [])].map((li) => li.getAttribute("aria-current"))).toEqual(["step"]);
  card()?.querySelector("button")?.click();
  flushSync();
  expect(card()).toBeNull();
  expect(JSON.parse(localStorage.getItem("ovra.hints") ?? "{}")).toMatchObject({ started: true, steps_hidden: true });
  unmount(busy);
});
