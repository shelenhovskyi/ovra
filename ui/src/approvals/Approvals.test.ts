import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { registerCommand } from "../keys/commands";
import type { PermissionRequest } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { ARM_MS, HOLD_MS } from "./queue.svelte";
import TestHost from "./TestHost.svelte";

const bash = (id: string, command = "ls -la"): PermissionRequest => ({
  id,
  session: "s1",
  project: "p1",
  cwd: "/demo",
  tool: "Bash",
  input: { command, description: `About ${command}` },
  title: null,
});

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

/** While set, no observed element comes into sight. */
let outOfSight = false;
/** While set, observed elements matching it do not come into sight. */
let hiddenPart: string | null = null;
let observers: FakeObserver[] = [];

/** jsdom lays nothing out: everything observed is in sight unless `outOfSight`. */
class FakeObserver {
  readonly targets = new Set<Element>();
  constructor(private readonly callback: IntersectionObserverCallback) {
    observers.push(this);
  }
  observe(element: Element): void {
    this.targets.add(element);
    if (!outOfSight && !(hiddenPart && element.matches(hiddenPart))) this.report(element);
  }
  report(element: Element): void {
    const entry = { target: element, isIntersecting: true, intersectionRatio: 1, intersectionRect: { height: 20 } };
    this.callback([entry as unknown as IntersectionObserverEntry], this as unknown as IntersectionObserver);
  }
  disconnect(): void {
    this.targets.clear();
  }
}

function comeIntoSight(): void {
  outOfSight = false;
  hiddenPart = null;
  for (const observer of observers) for (const element of observer.targets) observer.report(element);
}

/** Elements scrolled into view, with their options. */
let scrolled: [Element, unknown][] = [];

beforeEach(() => {
  vi.useFakeTimers();
  outOfSight = false;
  hiddenPart = null;
  observers = [];
  scrolled = [];
  vi.stubGlobal("IntersectionObserver", FakeObserver);
  Element.prototype.scrollIntoView = function (this: Element, options?: unknown) {
    scrolled.push([this, options]);
  };
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  vi.unstubAllGlobals();
  delete (Element.prototype as Partial<Element>).scrollIntoView;
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

/** Shows every card, not only the oldest. */
async function showRest() {
  await settle();
  target.querySelector<HTMLButtonElement>("[data-testid=approvals-more]")?.click();
  await settle();
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
/** Each task group's head: who asks, and the facts its requests share. */
const heads = () => [...target.querySelectorAll<HTMLElement>(".who")];
const words = (element: Element | undefined) => element?.textContent?.replace(/\s+/g, " ").trim();
/** Opens a card's details, as focusing it does. */
const expand = (card: HTMLElement) => card.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
const answers = (core: FakeAgentCore) => core.sent("permissions.answer").map((c) => c.params);

describe("Approvals", () => {
  it("says when a worker asks from its own copy of the project", async () => {
    const core = await setup((c) => {
      c.tasks = [
        {
          id: "t1",
          project: "p1",
          coordinator: "s9",
          workers: [{ session: "s1", name: "docs", branch: "ovra/t1/docs", worktree: "/wt/docs", kept: null }],
          limits: { max_workers: 3, budget_micro_usd: null },
          spent_micro_usd: 0,
        },
      ];
    });
    core.ask(bash("perm1"));
    await settle();
    // The window's own project goes without saying.
    expect(words(heads()[0]?.querySelector(".session") ?? undefined)).toBe("Worker docs");
    expect(words(cards()[0]?.querySelector(".worker") ?? undefined)).toBe("docs");
    expect(cards()[0]?.querySelector("[data-testid=approval-worker-copy]")?.textContent).toBe(
      "Own copy, branch ovra/t1/docs",
    );
  });

  it("says on the card when the session runs without a sandbox", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    const warning = () => heads()[0]?.querySelector("[data-testid=approval-unsandboxed]");
    // Not reported yet: no claim either way.
    expect(warning()).toBeNull();
    core.sandboxed("s1", null);
    await settle();
    expect(warning()?.textContent?.trim()).toBe("No sandbox");
    core.sandboxed("s1", { name: "bubblewrap", network: "open", gaps: [] });
    await settle();
    expect(warning()?.textContent?.trim()).toBe("Network open");
    core.sandboxed("s1", { name: "bubblewrap", network: "proxy", gaps: [] });
    await settle();
    expect(warning()).toBeNull();
  });

  it("says briefly on the card that the sandbox has gaps, with the list on request", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    const gaps = () => cards()[0]?.querySelector<HTMLDetailsElement>("[data-testid=approval-sandbox-gaps]");
    core.sandboxed("s1", { name: "bubblewrap", network: "proxy", gaps: [] });
    await settle();
    expect(gaps()).toBeNull();
    core.sandboxed("s1", {
      name: "fake sandbox",
      network: "proxy",
      gaps: ["DNS lookups", "loopback services"],
    });
    await settle();
    expect(gaps()?.querySelector("summary")?.textContent).toBe("Sandbox has gaps");
    expect(gaps()?.open).toBe(false);
    expect(gaps()?.querySelector("p")?.textContent).toBe("Not covered: DNS lookups, loopback services.");
    // Gaps alone are no reason for the stronger warning.
    expect(cards()[0]?.querySelector("[data-testid=approval-unsandboxed]")).toBeNull();
  });

  it("shows what the agent asks for without taking focus", async () => {
    const core = await setup();
    const elsewhere = target.querySelector<HTMLElement>("[data-testid=elsewhere]");
    elsewhere?.focus();
    core.ask(bash("perm1"));
    await settle();
    const card = cards()[0];
    expect(words(card?.querySelector("h3") ?? undefined)).toBe("Run ls -la");
    expect(card?.querySelector("h3 code")?.textContent).toBe("ls -la");
    expect(words(heads()[0])).toContain("Fix the typo");
    expect(card?.querySelector("[data-testid=approval-reason]")).toBeNull();
    core.ask({
      ...bash("perm2", "pwd"),
      reason: { text: "Path is outside allowed working directories", kind: "rule" },
    });
    await settle();
    await showRest();
    elsewhere?.focus();
    expect(cards()[1]?.querySelector("[data-testid=approval-reason]")?.textContent?.trim()).toBe(
      "Path is outside allowed working directories",
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
    // As for an early click, the card says why.
    expect(cards()[0]?.querySelector("[data-testid=approval-wait]")?.textContent?.trim()).toBe(
      "Ready in a moment — read the request first.",
    );
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("true");
    await settle(ARM_MS);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("false");
    // QWERTZ: the key labelled Z sits where Y is on QWERTY and does not allow.
    press("KeyY", { key: "z" });
    await settle();
    expect(answers(core)).toEqual([]);
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
    press("KeyZ", { key: "y" }); // QWERTZ: the key labelled Y
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
    // Held until the answer is sent: focus waits on the heading, so a
    // repeating Y types nothing where the person was.
    hold("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(document.activeElement?.id).toBe("approvals-heading");
    release("KeyY");
    // Up: focus goes back where it was, never to the page itself.
    expect(document.activeElement).toBe(elsewhere);
    core.ask(bash("perm2"));
    await settle(ARM_MS);
    expect(document.activeElement).toBe(elsewhere);
    for (const code of ["KeyC", "KeyA", "KeyR", "KeyR", "KeyY", "Space", "KeyA", "KeyN", "KeyD"]) press(code);
    await settle(ARM_MS);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
    press("Escape");
    expect(document.activeElement).toBe(elsewhere);
  });

  it("without a place to go back to, keeps focus on the heading after the last answer", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(document.activeElement?.id).toBe("approvals-heading");
    core.ask(bash("perm2"));
    await settle(ARM_MS);
    press("ArrowDown");
    expect(document.activeElement).toBe(cards()[0]);
  });

  it("after the last answer, holds focus on the heading instead of the task field", async () => {
    const core = await setup();
    const field = document.createElement("textarea");
    target.append(field);
    field.focus();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    const heading = target.querySelector("#approvals-heading");
    expect(document.activeElement).toBe(heading);
    // The follow-up arrives at once: Y and N on the heading answer nothing and say why.
    core.ask(bash("perm2", "pwd"));
    await settle(ARM_MS);
    for (const code of ["KeyY", "KeyN"]) {
      const down = new KeyboardEvent("keydown", {
        code,
        key: code.slice(3).toLowerCase(),
        bubbles: true,
        cancelable: true,
      });
      document.activeElement?.dispatchEvent(down);
      expect(down.defaultPrevented).toBe(true);
    }
    await settle(2 * ARM_MS);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    expect(document.activeElement).toBe(heading);
    expect(target.querySelector("[data-testid=approvals-notice]")?.textContent).toBe(
      "New request: press Alt+Shift+A to read it first.",
    );
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
  });

  it("gives the task field back a moment after the key is up, when nothing new came", async () => {
    const core = await setup();
    const field = document.createElement("textarea");
    target.append(field);
    field.focus();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    hold("KeyY");
    await settle(5000);
    expect(document.activeElement?.id).toBe("approvals-heading");
    release("KeyY");
    await settle(HOLD_MS - 1);
    expect(document.activeElement?.id).toBe("approvals-heading");
    await settle(1);
    expect(document.activeElement).toBe(field);
  });

  it("keeps Y and N out of a text field right after an answer", async () => {
    const core = await setup();
    const field = document.createElement("textarea");
    target.append(field);
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    await settle(ARM_MS);
    field.focus();
    // A click that leaves focus in the field, as in WebKit.
    cards()[0]?.querySelector<HTMLButtonElement>("button.deny")?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "deny" }]);
    expect(document.activeElement).toBe(field);
    const typeIn = (code: string) => {
      const down = new KeyboardEvent("keydown", {
        code,
        key: code.slice(3).toLowerCase(),
        bubbles: true,
        cancelable: true,
      });
      field.dispatchEvent(down);
      return down.defaultPrevented;
    };
    expect(typeIn("KeyN")).toBe(true);
    expect(typeIn("KeyT")).toBe(false);
    flushSync();
    expect(target.querySelector("[data-testid=approvals-notice]")?.textContent).toBe(
      "New request: press Alt+Shift+A to read it first.",
    );
    await settle(HOLD_MS);
    expect(typeIn("KeyN")).toBe(false);
    expect(answers(core)).toEqual([{ id: "perm1", decision: "deny" }]);
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

  it("shows the Y and N caps only while the keys answer", async () => {
    const core = await setup();
    const field = document.createElement("textarea");
    target.append(field);
    field.focus();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const caps = () => [...(cards()[0]?.querySelectorAll("kbd") ?? [])].map((k) => !k.classList.contains("off"));
    // Focus in a text field: Y and N would type there, so no cap promises them.
    expect(caps()).toEqual([false, false]);
    cards()[0]?.focus();
    flushSync();
    expect(caps()).toEqual([true, true]);
    // On a button inside the card the keys do nothing either.
    cards()[0]?.querySelector<HTMLButtonElement>("button.deny")?.focus();
    flushSync();
    expect(caps()).toEqual([false, false]);
    // The buttons answer a click wherever focus is.
    field.focus();
    cards()[0]?.querySelector<HTMLButtonElement>("button.deny")?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "deny" }]);
  });

  it("marks requests from a project other than the open one", async () => {
    const core = await setup();
    core.projects.push({ id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false });
    core.addSession("p2", "Elsewhere task", { state: "working" });
    core.status("s2", { state: "working" });
    core.ask(bash("perm1"));
    core.ask({ ...bash("perm2"), session: "s2", project: "p2", cwd: "/other" });
    await settle();
    await showRest();
    const other = heads().map((c) => c.querySelector("[data-testid=approval-other-project]") !== null);
    expect(other).toEqual([false, true]);
    expect(words(heads()[1])).toContain("Elsewhere task · other");
  });

  it("answers one request per key press, however long the key is held", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    core.ask(bash("perm3", "id"));
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
    core.ask({ ...bash("perm3"), tool: "Grep", input: { pattern: "TODO", path: "/demo/src" } });
    await showRest();
    for (const c of cards()) expand(c);
    const scopes = cards().map((c) => words(c.querySelector("[data-testid=approval-scope]") ?? undefined));
    // The title names the file once, by its path in the project; other paths are named on their line.
    expect(cards().map((c) => words(c.querySelector("h3") ?? undefined))).toEqual([
      "Edit README.md",
      "Read /demo/../etc/passwd",
      "Search TODO",
    ]);
    expect(cards()[0]?.querySelector("h3 code")?.getAttribute("title")).toBe("/demo/README.md");
    expect(scopes).toEqual(["In the project", "Outside the project", "In the project: src"]);
    // An edit is a small diff, not the tool's field names.
    const diff = cards()[0]?.querySelector("[data-testid=approval-diff]");
    expect([...(diff?.children ?? [])].map((line) => `${line.tagName} ${line.textContent}`)).toEqual([
      "DEL a",
      "INS b",
    ]);
    expect(words(cards()[0])).not.toMatch(/old string|new string|\/demo\/README/);
  });

  it("judges paths from the agent's own folder and warns about an unknown project", async () => {
    const core = await setup();
    core.ask({ ...bash("perm1"), tool: "Read", cwd: "/demo/.wt/w1", input: { file_path: "/demo/src/a.rs" } });
    core.ask({ id: "perm2", session: "s1", project: "p9", tool: "Read", input: { file_path: "/demo/a" }, title: null });
    await settle();
    await showRest();
    const lines = (testid: string) =>
      cards().map((c) => c.querySelector(`[data-testid=${testid}]`)?.textContent?.replace(/\s+/g, " ").trim());
    // A worker's worktree is its folder: the main checkout is outside it.
    expect(lines("approval-scope")).toEqual(["Outside the agent's folder", "Cannot tell if in a project"]);
    expect(cards().map((c) => words(c.querySelector("h3 code") ?? undefined))).toEqual(["/demo/src/a.rs", "/demo/a"]);
    expect(heads().map((h) => words(h.querySelector("[data-testid=approval-unknown-project]") ?? undefined))).toEqual([
      undefined,
      "Unknown project: paths cannot be checked",
    ]);
    expect(heads()[1]?.querySelector("[data-testid=approval-other-project]")).not.toBeNull();
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

  it("stacks identical requests into one card that answers them one at a time", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2"));
    await settle(ARM_MS);
    expect(cards().map((c) => c.dataset.request)).toEqual(["perm1"]);
    expect(words(cards()[0]?.querySelector("[data-testid=approval-stack]") ?? undefined)).toBe(
      "2 identical requests, answered one at a time",
    );
    expect(heads()).toHaveLength(1);
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    // The next one takes its place and waits again.
    expect(cards().map((c) => c.dataset.request)).toEqual(["perm2"]);
    expect(cards()[0]?.querySelector("button.allow")?.getAttribute("aria-disabled")).toBe("true");
  });

  it("waits again when the cards change order", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    await showRest();
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
    core.ask(bash("perm2", "pwd"));
    core.ask(bash("perm3", "rm -rf build"));
    // The rest open from the "more" line, which puts focus (and so the details) on the first of them.
    await showRest();
    await settle(ARM_MS);
    expect(cards().map((c) => c.querySelector("[data-details]") !== null)).toEqual([true, true, false]);
    const allow = cards()[2]?.querySelector<HTMLButtonElement>("button.allow");
    allow?.click();
    await settle();
    // The first press shows what it would allow; the cards below moved, so it waits again.
    expect(answers(core)).toEqual([]);
    expect(cards()[2]?.querySelector("[data-details]")?.textContent?.trim()).toBe("About rm -rf build");
    expect(allow?.getAttribute("aria-disabled")).toBe("true");
    await settle(ARM_MS);
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm3", decision: "allow" }]);
  });

  it("shows the oldest request whole and the rest on one line that opens them", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    core.ask(bash("perm3", "id"));
    await settle(ARM_MS);
    const more = () => target.querySelector<HTMLButtonElement>("[data-testid=approvals-more]");
    expect(cards().map((c) => c.dataset.request)).toEqual(["perm1"]);
    expect(more()?.textContent).toBe("2 more requests");
    // The heading still counts all of them.
    expect(target.querySelector("#approvals-heading")?.textContent).toContain("(3 waiting)");
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    // The next one takes the first place, and waits to be read.
    expect(cards().map((c) => c.dataset.request)).toEqual(["perm2"]);
    expect(document.activeElement).toBe(cards()[0]);
    expect(more()?.textContent).toBe("1 more request");
    await settle(ARM_MS);
    // Down from the last card shows the rest; what appeared waits again.
    press("ArrowDown");
    await settle();
    expect(more()).toBeNull();
    expect(document.activeElement).toBe(cards()[1]);
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("dims the window only for a card the keyboard reached, and shows more cards whole", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    await settle(ARM_MS);
    const section = target.querySelector<HTMLElement>("[data-testid=approvals]");
    const more = target.querySelector<HTMLButtonElement>("[data-testid=approvals-more]");
    more?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    more?.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    await settle();
    expect(document.activeElement).toBe(cards()[1]);
    expect(section?.classList.contains("keyed")).toBe(false);
    // From its top, under the group's head: not centred with its title cut off.
    expect(scrolled[0]).toEqual([cards()[1], { block: "start" }]);
    press("ArrowUp");
    flushSync();
    expect(section?.classList.contains("keyed")).toBe(true);
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
    // Only the heading keeps its room, and the notification is told at once.
    const section = target.querySelector("[data-testid=approvals]");
    expect(section?.classList.contains("collapsed")).toBe(true);
    const { approvalQueue } = await import("./queue.svelte");
    expect(approvalQueue(core).hidden).toBe(true);
    expect(target.querySelector("#approvals-heading")?.textContent).toContain("(1 waiting)");
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement).toBe(cards()[0]);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
  });

  it("focuses the request a notification is about", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    core.ask(bash("perm2", "pwd"));
    await settle();
    const { approvalQueue } = await import("./queue.svelte");
    // It waits behind the "more" line: its card comes up for it.
    approvalQueue(core).reveal("perm2");
    await settle();
    expect(document.activeElement).toBe(cards()[1]);
    // Its notification steps aside while the card has focus.
    expect(approvalQueue(core).focused).toBe("perm2");
    await settle();
    // All of it, not just the line that says who asks.
    expect(scrolled).toEqual([
      [cards()[1], { block: "start" }],
      [cards()[1]?.querySelector("[data-details]"), { block: "nearest" }],
    ]);
  });

  it("allows nothing whose details were never in sight, and scrolls them into view first", async () => {
    const core = await setup();
    outOfSight = true;
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const allow = () => cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    const wait = () => cards()[0]?.querySelector("[data-testid=approval-wait]")?.textContent?.trim();
    expect(allow()?.getAttribute("aria-disabled")).toBe("true");
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([]);
    expect(scrolled.map(([element]) => element)).toContain(cards()[0]?.querySelector("[data-details]"));
    expect(wait()).toBe("Scrolled to the request: read it, then allow.");
    // The click lands where the scroll moved things: it waits again too.
    comeIntoSight();
    await settle();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([]);
    await settle(ARM_MS);
    expect(allow()?.getAttribute("aria-disabled")).toBe("false");
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("allows nothing while who asks and the sandbox warning were never in sight", async () => {
    const core = await setup();
    hiddenPart = ".who";
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const allow = () => cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    expect(allow()?.getAttribute("aria-disabled")).toBe("true");
    cards()[0]?.focus();
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([]);
    // The card's top comes into view, not just its details.
    expect(scrolled.at(-1)).toEqual([cards()[0], { block: "start" }]);
    expect(cards()[0]?.querySelector("[data-testid=approval-wait]")?.textContent).toBe(
      "Scrolled to the request: read it, then allow.",
    );
    comeIntoSight();
    await settle(ARM_MS);
    press("KeyY");
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("shows the wait as a fill on Allow, and says why an early click does nothing", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    const allow = cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    const fill = () => allow?.querySelector<HTMLElement>("[data-testid=approval-arming]");
    const wait = () => cards()[0]?.querySelector("[data-testid=approval-wait]");
    expect(fill()?.style.animationDuration).toBe(`${ARM_MS}ms`);
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([]);
    // To screen readers only; on screen the fill says it, and stays in sight.
    expect(wait()?.textContent?.trim()).toBe("Ready in a moment — read the request first.");
    expect(wait()?.classList.contains("sr-only")).toBe(true);
    expect(scrolled.at(-1)).toEqual([allow, { block: "nearest" }]);
    // Return on a focused Allow never allows.
    await settle(ARM_MS);
    expect(fill()).toBeNull();
    const enter = new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true, cancelable: true });
    allow?.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(true);
    allow?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
  });

  it("after the last answer by pointer, gives focus back instead of staying over the agents", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle(ARM_MS);
    const allow = cards()[0]?.querySelector<HTMLButtonElement>("button.allow");
    allow?.focus();
    allow?.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow" }]);
    const section = target.querySelector<HTMLElement>("[data-testid=approvals]");
    expect(section?.classList.contains("idle")).toBe(true);
    expect(section?.contains(document.activeElement)).toBe(false);
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

  it("unfolds a folded Agents column for the shortcut", async () => {
    await setup();
    const column = document.createElement("div");
    column.dataset.testid = "agents";
    column.innerHTML = '<div class="strip"></div>';
    document.body.append(column);
    const unfold = vi.fn(() => column.querySelector(".strip")?.remove());
    const off = registerCommand({ id: "layout.toggle_agents", title: "t", run: unfold });
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    // Once: unfolded, the next press leaves the column as it is.
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(unfold).toHaveBeenCalledOnce();
    off();
    column.remove();
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

  it("takes no space while nothing waits, and shows itself when something does", async () => {
    const core = await setup();
    const section = () => target.querySelector<HTMLElement>("[data-testid=approvals]");
    const toggle = () => target.querySelector("button.toggle");
    // Out of sight, not out of the page: the shortcut and screen readers still reach it.
    expect(section()?.classList.contains("idle")).toBe(true);
    expect(toggle()).toBeNull();
    expect(section()?.textContent).toContain("Nothing waits for your approval.");
    press("KeyA", { altKey: true, shiftKey: true });
    await settle();
    expect(document.activeElement?.id).toBe("approvals-heading");

    core.ask(bash("perm1"));
    await settle();
    expect(section()?.classList.contains("idle")).toBe(false);
    expect(toggle()?.textContent).toBe("Hide");
    expect(target.querySelector("#approvals-heading")?.textContent).toContain("(1 waiting)");

    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "deny" } });
    await settle();
    expect(section()?.classList.contains("idle")).toBe(true);
  });

  it("stays in sight when it has something to report", async () => {
    const core = await setup();
    core.ask(bash("perm1"));
    await settle();
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "deny" } });
    await settle();
    const { approvalQueue } = await import("./queue.svelte");
    approvalQueue(core).reveal("perm1");
    await settle();
    const section = target.querySelector<HTMLElement>("[data-testid=approvals]");
    expect(target.querySelector("[data-testid=approvals-notice]")?.textContent).toBe(
      "That request was already answered.",
    );
    expect(section?.classList.contains("idle")).toBe(false);
  });
});

describe("Always allow", () => {
  const rule = { tool: "Bash", match: "npm test", action: "auto", exact: true, from_approval: true } as const;
  const always = () => cards()[0]?.querySelector<HTMLButtonElement>("[data-testid=approval-always]");

  it("adds the offered rule after the same wait and checks as Allow once, with no key of its own", async () => {
    const core = await setup();
    outOfSight = true;
    core.ask({ ...bash("perm1", "npm test"), always: rule });
    await settle();
    expect(words(always() ?? undefined)).toBe("Always allow npm test here");
    expect(always()?.querySelector("code")?.textContent).toBe("npm test");
    expect(always()?.hasAttribute("aria-keyshortcuts")).toBe(false);
    // What the command runs is not exact: one short line says so.
    const note = cards()[0]?.querySelector("[data-testid=approval-always-note]");
    expect(note?.textContent).toBe("Runs what this project's scripts say; the sandbox still applies.");
    expect(always()?.getAttribute("aria-describedby")).toBe(note?.id);
    always()?.click();
    await settle(ARM_MS);
    expect(answers(core)).toEqual([]);
    // Armed, but the details were never in sight: the press shows them first.
    expect(always()?.getAttribute("aria-disabled")).toBe("true");
    always()?.click();
    await settle();
    expect(answers(core)).toEqual([]);
    comeIntoSight();
    await settle(ARM_MS);
    expect(always()?.getAttribute("aria-disabled")).toBe("false");
    // Return never allows, not even always.
    always()?.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", bubbles: true, cancelable: true }));
    always()?.click();
    await settle();
    expect(answers(core)).toEqual([{ id: "perm1", decision: "allow", always: true }]);
  });

  it("says nothing about scripts for a file rule", async () => {
    const core = await setup();
    core.ask({
      ...bash("perm1"),
      tool: "Edit",
      input: { file_path: "/demo/src/a.ts", old_string: "a", new_string: "b" },
      always: { tool: "Edit", match: "src/a.ts", action: "auto", from_approval: true },
    });
    await settle();
    expect(words(always() ?? undefined)).toBe("Always allow src/a.ts here");
    expect(target.querySelector("[data-testid=approval-always-note]")).toBeNull();
    expect(always()?.hasAttribute("aria-describedby")).toBe(false);
  });

  it("is not offered when the core offers no rule or a path is not inside the folder", async () => {
    const core = await setup();
    core.ask(bash("perm1", "rm -rf .claude"));
    core.ask({
      ...bash("perm2"),
      tool: "Edit",
      input: { file_path: "/etc/hosts", old_string: "a", new_string: "b" },
      always: { tool: "Edit", match: "hosts", action: "auto", from_approval: true },
    });
    await showRest();
    expect(cards()).toHaveLength(2);
    expect(target.querySelector("[data-testid=approval-always]")).toBeNull();
  });
});
