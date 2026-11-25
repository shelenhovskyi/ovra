import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { approvalQueue } from "../approvals/queue.svelte";
import type { PermissionRequest } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import TestHost from "./TestHost.svelte";

const bash = (id: string): PermissionRequest => ({
  id,
  session: "s1",
  project: "p1",
  cwd: "/demo",
  tool: "Bash",
  input: { command: "ls" },
  title: null,
});

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  document.title = "Ovra";
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

describe("Notifications", () => {
  it("hides a request's toast while its card has focus", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    flushSync();
    core.status("s1", { state: "working" });
    core.ask(bash("perm1"));
    await new Promise((resolve) => setTimeout(resolve));
    await settle();
    const toasts = () =>
      [...target.querySelectorAll("[data-testid=toast]")].map((t) => t.querySelector("p")?.textContent);
    expect(toasts()).toEqual([expect.stringContaining("needs approval: Run ls")]);
    const queue = approvalQueue(core);
    queue.focused = "perm1";
    flushSync();
    expect(toasts()).toEqual([]);
    queue.focused = null;
    flushSync();
    expect(toasts()).toHaveLength(1);
  });

  it("says what waits as soon as the cards are hidden, and steps aside when they are shown", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    // A card in sight, as the approvals draw it.
    const section = document.createElement("section");
    section.dataset.testid = "approvals";
    const card = document.createElement("div");
    card.dataset.request = "perm1";
    card.getBoundingClientRect = () => DOMRect.fromRect({ x: 0, y: 40, width: 300, height: 200 });
    section.append(card);
    target.append(section);
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    flushSync();
    core.status("s1", { state: "working" });
    core.ask(bash("perm1"));
    await new Promise((resolve) => setTimeout(resolve));
    await settle();
    const toasts = () => target.querySelectorAll("[data-testid=toast]").length;
    expect(toasts()).toBe(0);
    const queue = approvalQueue(core);
    // Hide keeps focus on its button: no focus change is needed.
    queue.hidden = true;
    flushSync();
    expect(toasts()).toBe(1);
    queue.hidden = false;
    await settle();
    expect(toasts()).toBe(0);
  });

  it("puts a banner over a modal while approvals wait, and Show closes the modal", async () => {
    const core = new FakeAgentCore();
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    await settle();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");
    dialog.tabIndex = -1;
    let escaped = false;
    dialog.addEventListener("keydown", (event) => {
      if (event.code === "Escape") escaped = true;
    });
    target.append(dialog);
    dialog.focus();
    core.ask(bash("perm1"));
    await settle();
    const banner = () => target.querySelector("[data-testid=approvals-banner]");
    expect(banner()?.textContent?.replace(/\s+/g, " ").trim()).toBe("1 approval waits Show");
    // Nothing else about approvals while the modal covers them.
    expect(target.querySelectorAll("[data-testid=toast]")).toHaveLength(0);
    banner()?.querySelector("button")?.click();
    expect(escaped).toBe(true);
  });

  it("counts waiting requests in the window title", async () => {
    const core = new FakeAgentCore();
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    await settle();
    expect(document.title).toBe("Ovra");
    core.ask(bash("perm1"));
    await new Promise((resolve) => setTimeout(resolve));
    core.ask(bash("perm2"));
    await settle();
    expect(document.title).toBe("(2) Ovra");
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    await settle();
    expect(document.title).toBe("(1) Ovra");
    core.emit({ type: "permissions.resolved", data: { id: "perm2", decision: "deny" } });
    await settle();
    expect(document.title).toBe("Ovra");
  });
});
