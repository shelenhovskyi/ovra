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

const retries = () => [...document.querySelectorAll("button")].filter((b) => b.textContent === "Retry");

it("keeps Retry on a turn's error when only the turn's end line follows it", () => {
  const usage = { input_tokens: 40_000, output_tokens: 3_600, cost_micro_usd: null };
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [
        { kind: "user_text", text: "Fix it", from: "person" },
        { kind: "error", message: "The turn stopped: it reached its turn limit." },
        { kind: "turn_end", usage },
      ],
      onretry: () => {},
    },
  });
  flushSync();
  expect(document.querySelector(".divider")?.textContent).toContain("Turn ended");
  expect(retries()).toHaveLength(1);
});

it("names files relative to the session's folder, with the whole path on hover", () => {
  const file_path = "/tmp/ovra-ux/demo-app/docs/README.md";
  app = mount(Transcript, {
    target: document.body,
    props: {
      root: "/tmp/ovra-ux/demo-app",
      messages: [
        { kind: "tool_use", id: "t1", name: "Edit", input: { file_path, old_string: "teh", new_string: "the" } },
        { kind: "tool_result", tool_use_id: "t1", output: "ok", is_error: false },
      ],
    },
  });
  flushSync();
  const summary = document.querySelector(".step > summary");
  expect(summary?.textContent?.replace(/\s+/g, " ").trim()).toBe("Edited docs/README.md +1 −1");
  expect(summary?.getAttribute("title")).toBe(file_path);
  expect(summary?.querySelector("code")?.classList.contains("path")).toBe(true);
});

it("offers no Retry on an error that later messages answered", () => {
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [
        { kind: "error", message: "Overloaded" },
        { kind: "user_text", text: "Go on", from: "person" },
      ],
      onretry: () => {},
    },
  });
  flushSync();
  expect(retries()).toHaveLength(0);
});
