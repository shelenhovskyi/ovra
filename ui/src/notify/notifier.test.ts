import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { handleOpenFor } from "../inbox/open";
import { watchAgents } from "./agent-events";
import { INFO_MS, Notifier, overlaps, type SystemNotifications } from "./notifier.svelte";

function system(granted: boolean, foreground: boolean) {
  const shown: { title: string; body: string; click: () => void }[] = [];
  const closed: string[] = [];
  const api: SystemNotifications = {
    permission: () => (granted ? "granted" : "default"),
    request: async () => "granted",
    show: (title, body, _tag, click) => shown.push({ title, body, click }),
    close: (tag) => closed.push(tag),
    inForeground: () => foreground,
  };
  return { api, shown, closed };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("Notifier", () => {
  it("hides info after a while but keeps what needs the person", () => {
    const notifier = new Notifier(system(false, true).api);
    notifier.notify({ level: "info", text: "done" });
    notifier.notify({ level: "attention", key: "k", text: "needs you" });
    notifier.notify({ level: "attention", key: "k", text: "needs you again" });
    vi.advanceTimersByTime(INFO_MS);
    expect(notifier.toasts.map((t) => t.text)).toEqual(["needs you again"]);
    notifier.dismissKey("k");
    expect(notifier.toasts).toEqual([]);
  });

  it("uses system notifications only when allowed and Ovra is in the background", () => {
    const back = system(true, false);
    new Notifier(back.api).notify({ level: "info", text: "done" }, "Agent");
    expect(back.shown.map((s) => [s.title, s.body])).toEqual([["Agent", "done"]]);
    const front = system(true, true);
    new Notifier(front.api).notify({ level: "info", text: "done" });
    const off = system(false, false);
    new Notifier(off.api).notify({ level: "info", text: "done" });
    expect([...front.shown, ...off.shown]).toEqual([]);
  });

  it("tells whether the stack would cover a box", () => {
    const stack = { left: 0, right: 360, top: 320, bottom: 368 };
    expect(overlaps(stack, { left: 12, right: 628, top: 236, bottom: 441 })).toBe(true);
    expect(overlaps(stack, { left: 12, right: 628, top: 100, bottom: 320 })).toBe(false);
    // Nothing laid out covers nothing.
    expect(overlaps(stack, { left: 0, right: 0, top: 0, bottom: 0 })).toBe(false);
  });
});

describe("watchAgents", () => {
  it("notifies about approvals, finished turns and failures", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    const back = system(true, false);
    const notifier = new Notifier(back.api);
    const review = vi.fn();
    watchAgents(core, notifier, review);
    core.emit({ type: "agents.status", data: { session: "s1", project: "p1", status: { state: "starting" } } });

    core.ask({ id: "perm1", session: "s1", project: "p1", tool: "Bash", input: { command: "ls" }, title: null });
    await vi.advanceTimersByTimeAsync(0);
    // Named as the Tasks list names it once its title is known, never "Session s1".
    expect(notifier.toasts.map((t) => [t.level, t.text])).toEqual([
      ["attention", "Fix the typo needs approval: Run ls"],
    ]);
    back.shown[0]?.click();
    expect(review).toHaveBeenCalledExactlyOnceWith("perm1");
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    expect(notifier.toasts).toEqual([]);
    // Answered elsewhere: the system notification goes too, so its Review cannot be pressed late.
    expect(back.closed.at(-1)).toBe("permission:perm1");

    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    await vi.advanceTimersByTimeAsync(0);
    expect(notifier.toasts.map((t) => [t.level, t.text])).toEqual([["info", "Fix the typo is done."]]);
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    core.status("s1", { state: "failed", message: "crashed" });
    await vi.advanceTimersByTimeAsync(0);
    expect(notifier.toasts.map((t) => [t.level, t.text])).toEqual([["error", "Fix the typo failed: crashed"]]);
  });

  it("gives no toast for a request answered before its name was known", async () => {
    const core = new FakeAgentCore();
    const notifier = new Notifier(system(false, true).api);
    watchAgents(core, notifier, vi.fn());
    core.ask({ id: "perm1", session: "s9", project: "p1", tool: "Bash", input: { command: "ls" }, title: null });
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    await vi.advanceTimersByTimeAsync(0);
    expect(notifier.toasts).toEqual([]);
  });

  it("says what a direct session changed and offers Reviews", async () => {
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
        fingerprint: null,
        git_changes: [],
        git_unchecked: false,
      },
    ];
    const notifier = new Notifier(system(false, true).api);
    watchAgents(core, notifier, vi.fn());
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    await vi.advanceTimersByTimeAsync(0);
    expect(notifier.toasts.map((t) => [t.text, t.action?.label])).toEqual([
      ["Fix the typo is done: 1 file changed.", "Review"],
    ]);
  });

  it("says a task finished only when none of its agents works", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Add notes", { state: "working" });
    core.addSession("p1", "Write the notes", { state: "working" });
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: [{ session: "s2", name: "docs", branch: "ovra/t1/docs", worktree: "/w/docs", kept: null }],
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    const notifier = new Notifier(system(false, true).api);
    watchAgents(core, notifier, vi.fn());
    const texts = async () => {
      await vi.advanceTimersByTimeAsync(0);
      return notifier.toasts.map((t) => t.text);
    };
    core.status("s1", { state: "working" });
    core.status("s2", { state: "working" });
    // The coordinator handed out the work; its worker still runs.
    core.status("s1", { state: "idle" });
    expect(await texts()).toEqual([]);
    // Its worker waits for a free agent: nothing is done yet.
    core.status("s2", { state: "queued" });
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    expect(await texts()).toEqual([]);
    core.status("s2", { state: "working" });
    core.status("s2", { state: "idle" });
    expect(await texts()).toEqual(["Task: Add notes is done."]);
    // Its worker asked for review: the toast says so and opens Reviews.
    core.reviews = [
      {
        id: "r1",
        project: "p1",
        session: "s2",
        mode: "isolated",
        state: "open",
        summary: "Wrote notes",
        files: [
          {
            path: "NOTES.md",
            kind: "added",
            from: null,
            by: "agent",
            additions: 1,
            deletions: 0,
            blocked: false,
            protected: false,
          },
        ],
        can_undo: false,
        not_copied: [],
        can_undo_take: false,
        fingerprint: null,
        git_changes: [],
        git_unchecked: false,
      },
    ];
    core.status("s2", { state: "working" });
    core.status("s2", { state: "idle" });
    expect(await texts()).toEqual(["Add notes: 1 change to review"]);
    expect(notifier.toasts[0]?.action?.label).toBe("Review");
    // On the task's workers' items, not the one chosen last.
    const asked = vi.fn();
    const off = handleOpenFor(asked);
    void notifier.toasts[0]?.action?.run();
    off();
    expect(asked).toHaveBeenCalledWith(["s2"]);
    // Work starts again: the toast no longer holds.
    core.status("s2", { state: "working" });
    expect(await texts()).toEqual([]);
  });
});
