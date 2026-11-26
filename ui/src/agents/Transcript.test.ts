import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import Transcript from "./Transcript.svelte";

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
  document.body.replaceChildren();
});

it("starts unfaded at the top", () => {
  app = mount(Transcript, { target: document.body, props: { messages: [] } });
  flushSync();
  expect(document.querySelector("[role=log]")?.classList.contains("faded")).toBe(false);
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
  // Scrolled: its top line fades out instead of being cut through its letters.
  expect(log.classList.contains("faded")).toBe(true);
  log.scrollTop = 600;
  log.dispatchEvent(new Event("scroll"));
  flushSync();
  expect(jump()?.getAttribute("aria-label")).toBe("Jump to latest");
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

it("says the person stopped a turn, as no error and with no Retry", () => {
  const usage = { input_tokens: 40_000, output_tokens: 3_600, cost_micro_usd: null };
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [
        { kind: "user_text", text: "Run the tests", from: "person" },
        { kind: "interrupted" },
        { kind: "turn_end", usage },
        { kind: "user_text", text: "And again", from: "person" },
        // Saved before interrupts had their own kind.
        { kind: "error", message: "Interrupted." },
        { kind: "turn_end", usage },
      ],
      onretry: () => {},
    },
  });
  flushSync();
  const stopped = [...document.querySelectorAll(".interrupted")].map((li) => li.textContent?.trim());
  expect(stopped).toEqual(["You interrupted this turn.", "You interrupted this turn."]);
  expect(document.querySelector(".error")).toBeNull();
  expect(retries()).toHaveLength(0);
});

it("names files relative to the session's folder, with the whole path on hover", () => {
  const file_path = "/tmp/ovra-ux/demo-app/docs/README.md";
  app = mount(Transcript, {
    target: document.body,
    props: {
      root: "/tmp/ovra-ux/demo-app",
      messages: [
        {
          kind: "tool_use",
          id: "t1",
          name: "Edit",
          input: { file_path, old_string: "teh", new_string: "the" },
          call: { kind: "edit", paths: [file_path] },
        },
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

it("folds Ovra's notes in a row into one quiet line, and a tool's raw answer behind its plain one", () => {
  const note = (name: string) => ({
    kind: "user_text" as const,
    text: `Ovra: Worker ${name} asked for review.\nFor the coordinator: its agent_id is s-${name}.`,
    from: "ovra" as const,
  });
  app = mount(Transcript, {
    target: document.body,
    props: {
      agent: "Claude beta",
      messages: [
        { kind: "tool_use", id: "t1", name: "mcp__ovra__spawn_agent", input: { name: "docs", task: "Write docs." } },
        {
          kind: "tool_result",
          tool_use_id: "t1",
          output: '{"agent_id":"s2","branch":"ovra/t1/docs","name":"docs","worktree":"/wt/docs"}',
          is_error: false,
        },
        note("docs"),
        note("tests"),
        note("lint"),
        { kind: "user_text", text: "Ovra: Worker lint had its work approved by the person.", from: "ovra" },
        { kind: "assistant_text", text: "All three are done." },
        { kind: "user_text", text: "Ovra: Worker docs stopped.", from: "ovra" },
        { kind: "user_text", text: "Ovra: Worker lint failed: claude exited.", from: "ovra" },
      ],
    },
  });
  flushSync();
  const items = [...document.querySelectorAll("[data-testid=transcript] > li")];
  // No grey cards: one quiet line for each run of notes.
  expect(items.map((li) => li.className.split(" ")[0])).toEqual(["steps", "notes", "assistant_text", "notes"]);
  // In the person's words: what they did is "You".
  expect(items[1]?.querySelector("summary")?.textContent).toBe(
    "Workers docs, tests and lint asked for review · You kept lint",
  );
  expect([...(items[1]?.querySelectorAll(".cards > p:not(.detail)") ?? [])].map((p) => p.textContent)).toEqual([
    "Worker docs asked for review",
    "Worker tests asked for review",
    "Worker lint asked for review",
    "You kept lint",
  ]);
  expect(items[3]?.querySelector("summary")?.textContent).toBe(
    "Worker docs stopped · Worker lint failed: claude exited",
  );
  expect(items[1]?.querySelector("details")?.hasAttribute("open")).toBe(false);
  // The spawn result's JSON is folded under its plain sentence.
  const body = items[0]?.querySelector(".body");
  expect(body?.querySelector("p:not(.detail)")?.textContent).toBe(
    "Worker docs started on its own copy of the project (branch ovra/t1/docs)",
  );
  const raw = body?.querySelector("details");
  expect(raw?.querySelector("summary")?.textContent).toBe("What Claude beta got");
  expect(raw?.hasAttribute("open")).toBe(false);
});

it("says Not run for a refusal it only knows from the CLI's words, and not failed", () => {
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [
        {
          kind: "tool_use",
          id: "t1",
          name: "Bash",
          input: { command: "rm -rf build" },
          call: { kind: "execute", command: "rm -rf build" },
        },
        {
          kind: "tool_result",
          tool_use_id: "t1",
          output: "The user doesn't want to proceed with this tool use.",
          is_error: true,
        },
      ],
    },
  });
  flushSync();
  const step = document.querySelector(".step");
  expect(step?.querySelector("summary")?.textContent?.replace(/\s+/g, " ").trim()).toBe("Run rm -rf build Not run");
  expect(step?.classList.contains("failed")).toBe(false);
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

it("shows what the agent said as Markdown, and hands clicked links to onlink", () => {
  const onlink = vi.fn();
  app = mount(Transcript, {
    target: document.body,
    props: {
      messages: [{ kind: "assistant_text", text: "Done:\n\n- fixed `main.rs`\n- see [docs](https://example.com)" }],
      onlink,
    },
  });
  flushSync();
  const text = document.querySelector("[data-testid=agent-text]");
  expect(text?.querySelectorAll("li").length).toBe(2);
  expect(text?.querySelector("code")?.textContent).toBe("main.rs");
  text?.querySelector("a")?.click();
  expect(onlink).toHaveBeenCalledWith("https://example.com");
});
