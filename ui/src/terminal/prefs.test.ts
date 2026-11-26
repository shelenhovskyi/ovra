import { describe, expect, it } from "vitest";
import { FakeTerminals } from "./fake-terminals";
import { tabStore } from "./prefs";
import { TerminalSessions } from "./sessions.svelte";

function memory(initial: Record<string, string> = {}): Storage {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
    removeItem: (key) => void data.delete(key),
    clear: () => data.clear(),
    key: () => null,
    get length() {
      return data.size;
    },
  };
}

const SIZE = { cols: 80, rows: 24 };

describe("tabStore", () => {
  it("keeps each project's tabs apart and drops the entry when none are left", () => {
    const store = memory();
    const tabs = tabStore(store, memory());
    tabs.save("p1", { tabs: [{ id: "t1", title: "Terminal 1", cwd: "src" }], active: 0 });
    tabs.save("p2", { tabs: [{ id: null, title: "Terminal 1" }], active: 0 });
    expect(tabs.load("p1")).toEqual({ tabs: [{ id: "t1", title: "Terminal 1", cwd: "src" }], active: 0 });
    tabs.save("p1", null);
    expect(tabs.load("p1")).toBeNull();
    tabs.save("p2", null);
    tabs.leave();
    expect(store.length).toBe(0);
  });

  it("ignores a broken entry", () => {
    expect(tabStore(memory({ "ovra.terminal.tabs": "{not json" }), memory()).load("p1")).toBeNull();
    const odd = memory({ "ovra.terminal.tabs": '{"p1":{"tabs":[{"id":3,"title":"x"}]}}' });
    expect(tabStore(odd, memory()).load("p1")).toBeNull();
  });

  it("brings back tabs saved before they were kept per window", () => {
    const old = memory({ "ovra.terminal.tabs": '{"p1":{"tabs":[{"id":"t1","title":"Terminal 1"}],"active":0}}' });
    expect(tabStore(old, memory()).load("p1")).toEqual({ tabs: [{ id: "t1", title: "Terminal 1" }], active: 0 });
  });

  it("gives each window its own shells, and a reloaded window its own back", async () => {
    const local = memory();
    const core = new FakeTerminals();
    const windowA = memory();
    const a = tabStore(local, windowA);
    const sessionsA = new TerminalSessions(core, a);
    await sessionsA.start(sessionsA.add("p1"), SIZE);

    // A second window on the same project starts its own tab, and A keeps its list.
    const b = tabStore(local, memory());
    const sessionsB = new TerminalSessions(core, b);
    expect(sessionsB.restore("p1")).toBe(0);
    await sessionsB.start(sessionsB.add("p1"), SIZE);
    expect(a.load("p1")?.tabs).toEqual([{ id: "t1", title: "Terminal 1" }]);
    expect(b.load("p1")?.tabs).toEqual([{ id: "t2", title: "Terminal 1" }]);

    // A reloads: its session storage keeps its id, so it takes t1 back.
    a.leave();
    expect(tabStore(local, windowA).load("p1")?.tabs).toEqual([{ id: "t1", title: "Terminal 1" }]);
  });

  it("lets a new window take the tabs of the window left last, but not a duplicate", () => {
    const local = memory();
    const windowA = memory();
    const a = tabStore(local, windowA);
    a.save("p1", { tabs: [{ id: "t1", title: "Terminal 1" }], active: 0 });
    // A duplicated browser tab copies session storage, but A is still open.
    const copied = memory({ "ovra.terminal.window": windowA.getItem("ovra.terminal.window") ?? "" });
    expect(tabStore(local, copied).load("p1")).toBeNull();

    a.leave();
    expect(tabStore(local, memory()).load("p1")?.tabs).toEqual([{ id: "t1", title: "Terminal 1" }]);
    expect(tabStore(local, memory()).load("p1")).toBeNull();
  });
});
