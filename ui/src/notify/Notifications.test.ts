import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { approvalQueue } from "../approvals/queue.svelte";
import { focusMode } from "../focus/mode.svelte";
import type { PermissionRequest } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
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
  if (mainView.current) mainView.close(false);
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

  it("keeps toasts in Focus only while a project is open, as the shell shows Focus", async () => {
    focusMode.set(true);
    try {
      const core = new FakeAgentCore();
      core.addSession("p1", "Fix the typo", { state: "working" });
      const workspace = new Workspace();
      app = mount(TestHost, { target, props: { transport: core, workspace } });
      flushSync();
      core.status("s1", { state: "working" });
      core.ask(bash("perm1"));
      await new Promise((resolve) => setTimeout(resolve));
      await settle();
      const toasts = () => target.querySelectorAll("[data-testid=toast]").length;
      // The start screen: Focus waits, so the toast shows.
      expect(toasts()).toBe(1);
      workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
      await settle();
      expect(toasts()).toBe(0);
    } finally {
      focusMode.set(false);
    }
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

  it("takes a review toast away while Reviews is open", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    core.reviews = [
      {
        id: "r1",
        project: "p1",
        session: "s1",
        mode: "direct",
        state: "open",
        summary: null,
        files: [
          {
            path: "README.md",
            kind: "modified",
            from: null,
            by: "agent",
            additions: 1,
            deletions: 1,
            blocked: false,
            protected: false,
          },
        ],
        can_undo: false,
        not_copied: [],
        can_undo_take: false,
        can_undo_discard: false,
        can_undo_keep: false,
        can_publish_kept: false,
        fingerprint: null,
        git_changes: [],
        git_unchecked: false,
      },
    ];
    app = mount(TestHost, { target, props: { transport: core, workspace: new Workspace() } });
    flushSync();
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    await new Promise((resolve) => setTimeout(resolve));
    await settle();
    const toasts = () => [...target.querySelectorAll("[data-testid=toast] p")].map((p) => p.textContent);
    expect(toasts()).toEqual(["Fix the typo is done: 1 file changed."]);
    mainView.show("reviews");
    flushSync();
    expect(toasts()).toEqual([]);
    mainView.close(false);
    flushSync();
  });

  it("moves the stack to the top while Reviews is open, unless the top would cover focus", () => {
    app = mount(TestHost, { target, props: { transport: new FakeAgentCore(), workspace: new Workspace() } });
    flushSync();
    const stack = target.querySelector<HTMLElement>("[data-testid=notifications]");
    if (!stack) throw new Error("no stack");
    const atTop = () => stack.classList.contains("top");
    stack.getBoundingClientRect = () => DOMRect.fromRect({ x: 500, y: 0, width: 380, height: 60 });
    expect(atTop()).toBe(false);
    mainView.show("reviews");
    flushSync();
    expect(atTop()).toBe(true);

    // A focused control just under the title bar, where the stack would sit.
    const field = document.createElement("input");
    field.getBoundingClientRect = () => DOMRect.fromRect({ x: 600, y: 60, width: 200, height: 24 });
    target.append(field);
    field.focus();
    flushSync();
    expect(atTop()).toBe(false);
    // Focus moves on, out of the way: back to the top.
    const other = document.createElement("button");
    other.getBoundingClientRect = () => DOMRect.fromRect({ x: 0, y: 600, width: 80, height: 24 });
    target.append(other);
    other.focus();
    flushSync();
    expect(atTop()).toBe(true);

    mainView.close(false);
    flushSync();
    expect(atTop()).toBe(false);
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
