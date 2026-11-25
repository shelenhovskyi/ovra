import { describe, expect, it } from "vitest";
import { DEFAULT_WIDTH, Layout, MAX_WIDTH, MIN_WIDTH } from "./layout.svelte";

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

describe("Layout", () => {
  it("clamps widths, collapses, swaps and remembers all of it", () => {
    const store = memory();
    const layout = new Layout(store);
    expect(layout.side.width).toBe(DEFAULT_WIDTH.side);
    expect(layout.left).toBe("side");

    layout.resize("side", 9999);
    expect(layout.side.width).toBe(MAX_WIDTH);
    layout.resize("agents", 12);
    expect(layout.agents.width).toBe(MIN_WIDTH);
    layout.setCollapsed("agents", true);
    layout.swap();
    expect(layout.left).toBe("agents");

    const again = new Layout(store);
    expect(again.side).toEqual({ width: MAX_WIDTH, collapsed: false });
    expect(again.agents).toEqual({ width: MIN_WIDTH, collapsed: true });
    expect(again.swapped).toBe(true);
    again.reset("side");
    expect(again.side.width).toBe(DEFAULT_WIDTH.side);
    again.resetAll();
    expect(new Layout(store)).toMatchObject({
      side: { width: DEFAULT_WIDTH.side, collapsed: false },
      agents: { width: DEFAULT_WIDTH.agents, collapsed: false },
      swapped: false,
    });
  });

  it("starts from the defaults when the saved layout is broken or storage fails", () => {
    const broken = new Layout(memory({ "ovra.layout": "{not json" }));
    expect(broken.agents).toEqual({ width: DEFAULT_WIDTH.agents, collapsed: false });
    const odd = new Layout(memory({ "ovra.layout": '{"side":{"width":"wide"},"agents":{"width":5000}}' }));
    expect(odd.side.width).toBe(DEFAULT_WIDTH.side);
    expect(odd.agents.width).toBe(MAX_WIDTH);

    const failing = memory();
    failing.setItem = () => {
      throw new Error("quota");
    };
    const layout = new Layout(failing);
    expect(() => layout.swap()).not.toThrow();
    expect(layout.swapped).toBe(true);
  });
});
