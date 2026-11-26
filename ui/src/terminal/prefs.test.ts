import { describe, expect, it } from "vitest";
import { tabStore } from "./prefs";

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

describe("tabStore", () => {
  it("keeps each project's tabs apart and drops the entry when none are left", () => {
    const store = memory();
    const tabs = tabStore(store);
    tabs.save("p1", { tabs: [{ id: "t1", title: "Terminal 1", cwd: "src" }], active: 0 });
    tabs.save("p2", { tabs: [{ id: null, title: "Terminal 1" }], active: 0 });
    expect(tabStore(store).load("p1")).toEqual({ tabs: [{ id: "t1", title: "Terminal 1", cwd: "src" }], active: 0 });
    tabs.save("p1", null);
    expect(tabs.load("p1")).toBeNull();
    tabs.save("p2", null);
    expect(store.length).toBe(0);
  });

  it("ignores a broken entry", () => {
    expect(tabStore(memory({ "ovra.terminal.tabs": "{not json" })).load("p1")).toBeNull();
    const odd = memory({ "ovra.terminal.tabs": '{"p1":{"tabs":[{"id":3,"title":"x"}]}}' });
    expect(tabStore(odd).load("p1")).toBeNull();
  });
});
