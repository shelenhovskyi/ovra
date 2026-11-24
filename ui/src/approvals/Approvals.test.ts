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
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null };
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

function press(code: string, init: KeyboardEventInit = {}) {
  const key = code.startsWith("Key") ? code.slice(3).toLowerCase() : code;
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { code, key, bubbles: true, cancelable: true, ...init }),
  );
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
    expect(card?.textContent).toContain("Fix the typo asks to use Bash");
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
