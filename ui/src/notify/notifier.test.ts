import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { watchAgents } from "./agent-events";
import { INFO_MS, Notifier, type SystemNotifications } from "./notifier.svelte";

function system(granted: boolean, foreground: boolean) {
  const shown: { title: string; body: string; click: () => void }[] = [];
  const api: SystemNotifications = {
    permission: () => (granted ? "granted" : "default"),
    request: async () => "granted",
    show: (title, body, _tag, click) => shown.push({ title, body, click }),
    inForeground: () => foreground,
  };
  return { api, shown };
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
});

describe("watchAgents", () => {
  it("notifies about approvals, finished turns and failures", () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the typo", { state: "working" });
    const back = system(true, false);
    const notifier = new Notifier(back.api);
    const review = vi.fn();
    watchAgents(core, notifier, review);
    core.emit({ type: "agents.status", data: { session: "s1", project: "p1", status: { state: "starting" } } });

    core.ask({ id: "perm1", session: "s1", tool: "Bash", input: { command: "ls" }, title: null });
    expect(notifier.toasts.map((t) => [t.level, t.text])).toEqual([["attention", "Session s1 needs approval: Run ls"]]);
    back.shown[0]?.click();
    expect(review).toHaveBeenCalledOnce();
    core.emit({ type: "permissions.resolved", data: { id: "perm1", decision: "allow" } });
    expect(notifier.toasts).toEqual([]);

    core.status("s1", { state: "working" });
    core.status("s1", { state: "idle" });
    core.status("s1", { state: "failed", message: "crashed" });
    expect(notifier.toasts.map((t) => [t.level, t.text])).toEqual([["error", "Session s1 failed: crashed"]]);
  });
});
