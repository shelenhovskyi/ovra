import { describe, expect, it } from "vitest";
import { AgentPrefs } from "./prefs.svelte";

function memory(): Pick<Storage, "getItem" | "setItem"> {
  const items = new Map<string, string>();
  return { getItem: (k) => items.get(k) ?? null, setItem: (k, v) => void items.set(k, v) };
}

describe("AgentPrefs", () => {
  it("hides resource usage until the person turns it on, and keeps the choice", () => {
    const store = memory();
    const prefs = new AgentPrefs(store);
    expect(prefs.showResources).toBe(false);
    prefs.showResources = true;
    expect(new AgentPrefs(store).showResources).toBe(true);
  });

  it("falls back to off when storage is unreadable or missing", () => {
    const broken = { getItem: () => "{not json", setItem: () => {} };
    expect(new AgentPrefs(broken).showResources).toBe(false);
    const prefs = new AgentPrefs(null);
    prefs.showResources = true;
    expect(prefs.showResources).toBe(true);
  });

  it("offers attempts only once the person turns them on, without touching the other choice", () => {
    const store = memory();
    const prefs = new AgentPrefs(store);
    expect(prefs.attempts).toBe(false);
    prefs.showResources = true;
    prefs.attempts = true;
    const again = new AgentPrefs(store);
    expect([again.attempts, again.showResources]).toEqual([true, true]);
  });
});
