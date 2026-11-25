import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import Transcript from "./Transcript.svelte";

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
  document.body.replaceChildren();
});

it("stops following once scrolled up, offers Jump to latest, and Retry on a turn's error", () => {
  const onretry = vi.fn();
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [
        { kind: "user_text", text: "Fix it", from: "person" },
        { kind: "error", message: "Overloaded" },
      ],
      onretry,
    },
  });
  flushSync();
  const log = document.querySelector<HTMLElement>("[role=log]");
  expect(log?.getAttribute("aria-label")).toBe("Transcript");
  // jsdom lays nothing out: give the log a height; 40 px from the end still follows.
  if (!log) throw new Error("no log");
  Object.defineProperty(log, "scrollHeight", { value: 1000 });
  Object.defineProperty(log, "clientHeight", { value: 200 });
  log.scrollTop = 760;
  log.dispatchEvent(new Event("scroll"));
  flushSync();
  const jump = () => document.querySelector<HTMLButtonElement>("[data-testid=jump-to-latest]");
  expect(jump()).toBeNull();
  log.scrollTop = 600;
  log.dispatchEvent(new Event("scroll"));
  flushSync();
  expect(jump()?.textContent).toBe("Jump to latest");
  jump()?.click();
  flushSync();
  expect(log.scrollTop).toBe(1000);
  expect(jump()).toBeNull();
  [...document.querySelectorAll("button")].find((b) => b.textContent === "Retry")?.click();
  expect(onretry).toHaveBeenCalledOnce();
});
