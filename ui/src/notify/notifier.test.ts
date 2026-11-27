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

  it("keeps toasts that point to Reviews out of sight while Reviews is open", () => {
    const { api, shown } = system(true, false);
    const notifier = new Notifier(api);
    const review = { key: "task:t1", level: "info" as const, text: "Add notes: 3 changes to review", review: true };
    notifier.notify(review);
    notifier.notify({ level: "error", text: "lint failed" });
    notifier.stepAsideForReviews(true);
    expect(notifier.toasts.map((t) => t.text)).toEqual(["lint failed"]);
    // A count that changes there (after a Keep) does not come back on top of it.
    notifier.notify({ ...review, text: "Add notes: 2 changes to review" });
    expect(notifier.toasts.map((t) => t.text)).toEqual(["lint failed"]);
    // In the background, the system still says it.
    expect(shown.map((n) => n.body)).toContain("Add notes: 2 changes to review");
    notifier.stepAsideForReviews(false);
    notifier.notify({ ...review, text: "Add notes: 1 change to review" });
    expect(notifier.toasts.map((t) => t.text)).toEqual(["lint failed", "Add notes: 1 change to review"]);
  });

  it("shows only errors while Focus is on; the system still speaks in the background", () => {
    const { api, shown } = system(true, false);
    const notifier = new Notifier(api);
    notifier.notify({ level: "attention", key: "approvals", text: "1 request needs approval" });
    notifier.quietForFocus(true);
    expect(notifier.toasts).toEqual([]);
    notifier.notify({ level: "info", text: "Fix sync is done" });
    notifier.notify({ level: "error", text: "Could not start the agent" });
    expect(notifier.toasts.map((t) => t.text)).toEqual(["Could not start the agent"]);
    expect(shown.map((n) => n.body)).toContain("Fix sync is done");
    notifier.quietForFocus(false);
    notifier.notify({ level: "info", text: "Add a test is done" });
    expect(notifier.toasts.map((t) => t.text)).toEqual(["Could not start the agent", "Add a test is done"]);
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
    // A second request: still one toast, with the count; one answered: the count goes down quietly.
    core.ask({ id: "perm2", session: "s1", project: "p1", tool: "Bash", input: { command: "pwd" }, title: null });
    await vi.advanceTimersByTimeAsync(0);
    expect(notifier.toasts.map((t) => t.text)).toEqual(["2 requests need approval"]);
    const systemShown = back.shown.length;
    core.emit({ type: "permissions.resolved", data: { id: "perm2", decision: "allow" } });
    expect(notifier.toasts.map((t) => t.text)).toEqual(["Fix the typo needs approval: Run ls"]);
    expect(back.shown).toHaveLength(systemShown);
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    expect(notifier.toasts).toEqual([]);
    // Answered elsewhere: the system notification goes too, so its Review cannot be pressed late.
    expect(back.closed.at(-1)).toBe("permissions");

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

  it("takes back a done toast when the session works again, and never calls a failed turn done", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    const notifier = new Notifier(system(false, true).api);
    watchAgents(core, notifier, vi.fn());
    const toasts = async () => {
      await vi.advanceTimersByTimeAsync(0);
      return notifier.toasts.map((t) => [t.level, t.text]);
    };
    core.status("s1", { state: "starting" });
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    expect(await toasts()).toEqual([["info", "Fix the typo is done."]]);
    // A reply: it asks again. "Done" is no longer true.
    core.status("s1", { state: "working" });
    expect(await toasts()).toEqual([]);
    core.status("s1", { state: "waiting_for_approval" });
    core.status("s1", { state: "working" });
    // The turn ends with an error, as claude ends any turn.
    core.message("s1", { kind: "error", message: "Reached the turn limit." });
    core.status("s1", { state: "idle" });
    expect(await toasts()).toEqual([["error", "Fix the typo stopped: Reached the turn limit."]]);
    core.status("s1", { state: "working" });
    expect(await toasts()).toEqual([]);
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
        can_undo_discard: false,
        can_undo_keep: false,
        can_publish_kept: false,
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
    expect(notifier.toasts.map((t) => [t.text, t.action?.label, t.review])).toEqual([
      ["Fix the typo is done: 1 file changed.", "Review", true],
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
        can_undo_discard: false,
        can_undo_keep: false,
        can_publish_kept: false,
        fingerprint: null,
        git_changes: [],
        git_unchecked: false,
      },
    ];
    core.status("s2", { state: "working" });
    core.status("s2", { state: "idle" });
    expect(await texts()).toEqual(["Add notes: 1 change to review"]);
    expect(notifier.toasts[0]?.action?.label).toBe("Review");
    expect(notifier.toasts[0]?.review).toBe(true);
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

  it("never calls a task done while its workers are prepared or their setup failed", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Add notes", { state: "working" });
    const names = ["docs", "tests", "lint"];
    for (const n of names) core.addSession("p1", `Write ${n}`, { state: "preparing" });
    const setup = { step: 0, steps: 1, command: "pnpm install", log: [], failed: null as string | null };
    core.tasks.push({
      id: "t1",
      project: "p1",
      coordinator: "s1",
      workers: names.map((name, i) => ({
        session: `s${i + 2}`,
        name,
        branch: `ovra/t1/${name}`,
        worktree: `/w/${name}`,
        kept: null,
        setup: { ...setup },
      })),
      limits: { max_workers: 3, budget_micro_usd: null },
      spent_micro_usd: 0,
    });
    const notifier = new Notifier(system(false, true).api);
    watchAgents(core, notifier, vi.fn());
    const toasts = async () => {
      await vi.advanceTimersByTimeAsync(0);
      return notifier.toasts.map((t) => [t.level, t.text]);
    };
    // Their setups still run: the task is under way.
    core.status("s1", { state: "idle" });
    expect(await toasts()).toEqual([]);
    // Two failed, one still runs.
    const workers = core.tasks[0]?.workers ?? [];
    for (const w of workers.slice(0, 2)) w.setup = { ...setup, failed: "exited with code 1" };
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    expect(await toasts()).toEqual([]);
    // All failed: the task waits on the person, it is not done.
    for (const w of workers) w.setup = { ...setup, failed: "exited with code 1" };
    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    expect(await toasts()).toEqual([["attention", "Add notes: setup failed for 3 workers"]]);
  });
});
