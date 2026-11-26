// Its own file: hints are remembered per module, and FirstSteps.svelte.test.ts marks them.
import { flushSync, mount, unmount } from "svelte";
import { expect, it } from "vitest";
import FirstSteps from "./FirstSteps.svelte";

it("says when a task ended with nothing to review, and reading it checks the work", () => {
  const opened: string[] = [];
  const props = $state<{
    agent: string;
    ready: boolean;
    started: boolean;
    ended?: { failed: boolean; open: () => void };
  }>({
    agent: "Claude",
    ready: true,
    started: true,
    ended: { failed: true, open: () => opened.push("failed") },
  });
  const app = mount(FirstSteps, { target: document.body, props });
  const detail = () => document.querySelector("[data-testid=first-steps-detail]")?.textContent;
  const button = () => document.querySelector<HTMLButtonElement>("[data-testid=first-steps] li[aria-current] button");
  expect(detail()).toBe("The task stopped with an error.");
  props.ended = { failed: false, open: () => opened.push("answer") };
  flushSync();
  expect(detail()).toBe("Claude answered.");
  expect(button()?.textContent).toBe("Read");
  button()?.click();
  flushSync();
  expect(opened).toEqual(["answer"]);
  expect(document.querySelector("[data-testid=first-steps]")).toBeNull();
  unmount(app);
});
