import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import type { PermissionRequest } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { ARM_MS } from "./queue.svelte";
import TestHost from "./TestHost.svelte";

const bash = (id: string, command = "ls -la"): PermissionRequest => ({
  id,
  session: "s1",
  tool: "Bash",
  input: { command },
  title: null,
});

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  vi.useRealTimers();
});

async function setup() {
  const core = new FakeAgentCore();
  core.addSession("p1", "Fix the typo", { state: "working" });
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  core.status("s1", { state: "working" }); // the session becomes known, then its title
  await settle();
  return core;
}

async function settle(ms = 0) {
  flushSync();
  await vi.advanceTimersByTimeAsync(ms);
  flushSync();
}

/** Presses a key down without releasing it. */
function hold(code: string, init: KeyboardEventInit = {}) {
  const key = code.startsWith("Key") ? code.slice(3).toLowerCase() : code;
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { code, key, bubbles: true, cancelable: true, ...init }),
  );
}

function release(code: string) {
  window.dispatchEvent(new KeyboardEvent("keyup", { code, bubbles: true }));
}

/** A tap: down and up. */
function press(code: string, init: KeyboardEventInit = {}) {
  hold(code, init);
  release(code);
}

const cards = () => [...target.querySelectorAll<HTMLElement>("[data-request]")];
const answers = (core: FakeAgentCore) => core.sent("permissions.answer").map((c) => c.params);

describe("Approvals", () => {
  it("shows what the agent asks for without taking focus", async () => {
    const core = await setup();
    const elsewhere = target.querySelector<HTMLElement>("[data-testid=elsewhere]");
    elsewhere?.focus();
    core.ask(bash("perm1"));
    await settle();
    const card = cards()[0];
    expect(card?.querySelector("h3")?.textContent).toBe("Run ls -la");
    expect(card?.textContent?.replace(/\s+/g, " ")).toContain("Fix the typo in demo asks to use Bash");
    expect(card?.querySelector("[data-testid=approval-reason]")).toBeNull();
    core.ask({ ...bash("perm2"), reason: { text: "Path is outside allowed working directories", kind: "rule" } });
    await settle();
    expect(cards()[1]?.querySelector("[data-testid=approval-reason]")?.textContent?.trim()).toBe(
      "Why it asks: Path is outside allowed working directories (rule)",
    );
    expect(document.activeElement).toBe(elsewhere);
    press("KeyY");
    press("Enter");
    await settle(ARM_MS);
    expect(answers(core)).toEqual([]);
  });

  it("answers only after the request has been on screen for a moment", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([]);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("true");
    await settle(ARM_MS);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("false");
    // A non-Latin layout: the physical Y key still allows.
    press("KeyY", { key: "\u043d" }); // Cyrillic en
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("moves to the next request, which a second press does not answer", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "rm -rf build"));
    await settle(ARM_MS);
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(document.activeElement).toBe(cards()[0]);
    expect(cards()[0]?.dataset.request).toBe("perm2");
    await settle(ARM_MS);
    press("KeyN");
    await settle();
    expect(answers(core)).toEqual([
      { id: "perm1", decision: "allow" },
      { id: "perm2", decision: "deny" },
    ]);
  });

  it("waits in the list for the next request after the last answer", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    const elsewhere = target.querySelector<HTMLElement>("[data-testid=elsewhere]");
    elsewhere?.focus();
    press("KeyA", { altKey: true, shiftKey: true });
    await settle(ARM_MS);
    press("KeyY");
    await settle();
    expect(document.activeElement?.id).toBe("approvals-heading");
    core.ask(bash("perm2"));
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
    press("KeyY"); // too soon after it appeared
    await settle(ARM_MS);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    press("Escape");
    expect(document.activeElement).toBe(elsewhere);
  });

  it("answers one request per key press, however long the key is held", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2"));
    core.ask(bash("perm3"));
    await settle(ARM_MS);
    cards()[0]?.focus();
    hold("KeyY");
    // 2.5 s of auto-repeat, as in the UX report.
    for (let i = 0; i < 20; i++) {
      await settle(125);
      hold("KeyY", { repeat: true });
    }
    // Some systems repeat without the flag: still no keyup, still one answer.
    hold("KeyY");
    await settle(ARM_MS);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("true");
    release("KeyY");
    await settle(ARM_MS);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("false");
    press("KeyY");
    await settle();
    expect(answers(core).map((a) => a.id)).toEqual(["perm1", "perm2"]);
  });

  it("does not let a click land on a card that just moved under the pointer", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "rm -rf build"));
    await settle(ARM_MS);
    // Another window answers the first one; the second moves up.
    core.pending = core.pending.filter((r) => r.id !== "perm1");
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    await settle();
    const allow = cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    expect(allow?.getAttribute("aria-disabled")).toBe("true");
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([]);
    await settle(ARM_MS);
    expect(allow?.getAttribute("aria-disabled")).toBe("false");
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm2", decision: "allow" }]);
  });

  it("looks disabled exactly while it refuses, also long after the page opened", async () => {
    const core = await setup();
    await settle(10 * 60_000);
    core.ask(bash("perm1"));
    await settle();
    const allow = () => cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    expect(allow()?.getAttribute("aria-disabled")).toBe("true");
    await settle(ARM_MS);
    expect(allow()?.getAttribute("aria-disabled")).toBe("false");
    allow()?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("names the project and whether each path is inside it", async () => {
    const core = await setup();
    core.ask({
      ...bash("perm1"),
      tool: "Edit",
      input: { file_path: "/demo/README.md", old_string: "a", new_string: "b" },
    });
    core.ask({ ...bash("perm2"), tool: "Read", input: { file_path: "/demo/../etc/passwd" } });
    await settle();
    const scopes = cards().map((c) => c.querySelector("[data-testid=approval-scope]")?.textContent?.trim());
    expect(scopes).toEqual([
      "Inside the project folder: /demo/README.md",
      "⚠ Outside the project folder: /demo/../etc/passwd",
    ]);
  });

  it("focuses the request a notification is about", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2"));
    await settle();
    const { approvalQueue } = await import("./queue.svelte");
    approvalQueue(core).reveal("perm2");
    expect(document.activeElement).toBe(cards()[1]);
  });

  it("denies with a message from Shift+N", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    cards()[0]?.focus();
    press("KeyN", { shiftKey: true });
    await settle();
    const field = document.activeElement as HTMLTextAreaElement;
    expect(field.tagName).toBe("TEXTAREA");
    field.value = "use git ls-files";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    press("KeyY"); // typing in the message never allows
    press("Enter", { ctrlKey: true });
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "deny", message: "use git ls-files" }]);
  });

  it("is reached with a shortcut and Escape goes back", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    const elsewhere = target.querySelector<HTMLElement>("[data-testid=elsewhere]");
    elsewhere?.focus();
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
    press("Escape");
    expect(document.activeElement).toBe(elsewhere);
  });

  it("removes a request whose session ended", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "deny" } });
    await settle();
    expect(cards()).toEqual([]);
    expect(target.textContent).toContain("Nothing waits for your approval.");
  });
});
