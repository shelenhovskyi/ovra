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
  project: "p1",
  cwd: "/demo",
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

async function setup(prepare: (core: FakeAgentCore) => void = () => {}) {
  const core = new FakeAgentCore();
  prepare(core);
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
  it("says when a worker asks from its own copy of the project", async () => {
    const core = await setup((c) => {
      c.tasks = [
        {
          id: "t1",
          project: "p1",
          coordinator: "s9",
          workers: [{ session: "s1", name: "docs", branch: "ovra/t1/docs", worktree: "/wt/docs" }],
          limits: { max_workers: 3, budget_micro_usd: null },
          spent_micro_usd: 0,
        },
      ];
    });
    core.ask(bash("perm1"));
    await settle();
    expect(cards()[0]?.querySelector(".session")?.textContent?.replace(/\s+/g, " ")).toContain("Worker docs in demo");
    expect(cards()[0]?.querySelector("[data-testid=approval-worker-copy]")?.textContent).toBe(
      "Works in its own copy of the project, on branch ovra/t1/docs.",
    );
  });

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
      "Why it asks: Path is outside allowed working directories",
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

  it("never moves focus to a request that arrives, so typing cannot answer it", async () => {
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
    await settle(ARM_MS);
    expect(document.activeElement?.id).toBe("approvals-heading");
    for (const code of ["KeyC", "KeyA", "KeyR", "KeyR", "KeyY", "Space", "KeyA", "KeyN", "KeyD"]) press(code);
    await settle(ARM_MS);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    press("ArrowDown");
    expect(document.activeElement).toBe(cards()[0]);
    press("Escape");
    expect(document.activeElement).toBe(elsewhere);
  });

  it("ignores Y and N typed in a text field", async () => {
    const core = await setup();
    const field = document.createElement("textarea");
    target.append(field);
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    field.focus();
    for (const code of ["KeyY", "KeyN"]) press(code);
    await settle();
    expect(answers(core)).toEqual([]);
    expect(document.activeElement).toBe(field);
  });

  it("marks requests from a project other than the open one", async () => {
    const core = await setup();
    core.projects.push({ id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false });
    core.addSession("p2", "Elsewhere task", { state: "working" });
    core.status("s2", { state: "working" });
    core.ask(bash("perm1"));
    core.ask({ ...bash("perm2"), session: "s2", project: "p2", cwd: "/other" });
    await settle();
    const other = cards().map((c) => c.querySelector("[data-testid=approval-other-project]") !== null);
    expect(other).toEqual([false, true]);
    expect(cards()[1]?.textContent?.replace(/\s+/g, " ")).toContain("Elsewhere task in other asks to use Bash");
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
      "Inside the project folder: README.md (/demo/README.md)",
      "⚠ Outside the project folder: /demo/../etc/passwd",
    ]);
  });

  it("judges paths from the agent's own folder and warns about an unknown project", async () => {
    const core = await setup();
    core.ask({ ...bash("perm1"), tool: "Read", cwd: "/demo/.wt/w1", input: { file_path: "/demo/src/a.rs" } });
    core.ask({ id: "perm2", session: "s1", project: "p9", tool: "Read", input: { file_path: "/demo/a" }, title: null });
    await settle();
    const lines = (testid: string) =>
      cards().map((c) => c.querySelector(`[data-testid=${testid}]`)?.textContent?.replace(/\s+/g, " ").trim());
    // A worker's worktree is its folder: the main checkout is outside it.
    expect(lines("approval-scope")).toEqual([
      "⚠ Outside the agent's working folder: /demo/src/a.rs",
      "⚠ Cannot tell whether this is inside a project: /demo/a",
    ]);
    expect(lines("approval-unknown-project")).toEqual([
      undefined,
      "⚠ Project unknown: Ovra cannot tell whether these paths are inside a project.",
    ]);
    expect(cards()[1]?.querySelector("[data-testid=approval-other-project]")).not.toBeNull();
    // The core does not know p9: asking again for every card would flood it.
    const asked = core.sent("projects.list").length;
    core.ask({ id: "perm3", session: "s1", project: "p9", tool: "Read", input: { file_path: "/a" }, title: null });
    await settle();
    expect(core.sent("projects.list").length).toBe(asked);
  });

  it("answers Y and N only on the card itself, not from a button on it", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    cards()[0]?.querySelector<HTMLButtonElement>("button:not(.allow)")?.focus();
    press("KeyY");
    press("KeyN");
    await settle(ARM_MS);
    expect(answers(core)).toEqual([]);
  });

  it("after an answer, focuses only a request that was already on screen", async () => {
    const core = await setup();
    const call = core.call.bind(core);
    let deliver = () => {};
    vi.spyOn(core, "call").mockImplementation(async (request) => {
      if (request.method === "permissions.answer") await new Promise<void>((done) => (deliver = done));
      return call(request);
    });
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    core.ask(bash("perm2", "rm -rf build"));
    deliver();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(document.activeElement?.id).toBe("approvals-heading");
  });

  it("waits again when the cards change order", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2"));
    await settle(ARM_MS);
    core.pending.reverse();
    core.emit({ type: "info.resync", data: { skipped: 2 } });
    await settle();
    expect(cards().map((c) => c.dataset.request)).toEqual(["perm2", "perm1"]);
    const allow = cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    expect(allow?.getAttribute("aria-disabled")).toBe("true");
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([]);
  });

  it("says so when a notification's request was already answered", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const { approvalQueue } = await import("./queue.svelte");
    approvalQueue(core).reveal("perm9");
    await settle();
    expect(document.activeElement?.id).toBe("approvals-heading");
    expect(target.querySelector("[data-testid=approvals-notice]")?.textContent).toBe(
      "That request was already answered.",
    );
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([]);
  });

  it("shows only the first request's details and never allows one unseen", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "rm -rf build"));
    await settle(ARM_MS);
    expect(cards().map((c) => c.querySelector("dl") !== null)).toEqual([true, false]);
    const allow = cards()[1]?.querySelector<HTMLButtonElement>("button.allow");
    allow?.click();
    await settle();
    // The first press shows what it would allow; the cards below moved, so it waits again.
    expect(answers(core)).toEqual([]);
    expect(cards()[1]?.querySelector("dl pre")?.textContent).toBe("rm -rf build");
    expect(allow?.getAttribute("aria-disabled")).toBe("true");
    await settle(ARM_MS);
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm2", decision: "allow" }]);
  });

  it("can be hidden, and opens again for the shortcut", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const toggle = target.querySelector<HTMLButtonElement>("button.toggle");
    toggle?.click();
    await settle();
    expect(cards()).toEqual([]);
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(target.querySelector("#approvals-heading")?.textContent).toContain("(1 waiting)");
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
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
