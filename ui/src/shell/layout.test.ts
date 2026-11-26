import { describe, expect, it } from "vitest";
import { DEFAULT_WIDTH, Layout, MIN_DOCK, MIN_WIDTH } from "./layout.svelte";

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
    layout.viewport = 1440;
    expect(layout.side.width).toBe(DEFAULT_WIDTH.side);
    expect(layout.left).toBe("side");

    // Up to 60% of the window, as long as the main area keeps 320 px beside Agents.
    layout.resize("side", 9999);
    expect(layout.side.width).toBe(1440 - 52 - 16 - 320 - 320);
    layout.viewport = 2000;
    layout.resize("side", 9999);
    expect(layout.side.width).toBe(1200);
    // A narrower window draws it narrower, and remembers the wish.
    layout.viewport = 1000;
    expect(layout.width("side")).toBe(1000 - 52 - 16 - 320 - 320);
    layout.resize("agents", 12);
    expect(layout.agents.width).toBe(MIN_WIDTH);
    layout.setCollapsed("agents", true);
    layout.swap();
    expect(layout.left).toBe("agents");

    const again = new Layout(store);
    expect(again.side).toEqual({ width: 1200, collapsed: false });
    expect(again.agents).toEqual({ width: MIN_WIDTH, collapsed: true });
    expect(again.swapped).toBe(true);
    // The split between the editor groups, kept within bounds.
    again.resizeGroups(0.3);
    expect(new Layout(store).groups).toBe(0.3);
    again.resizeGroups(2);
    expect(again.groups).toBe(0.9);
    again.reset("side");
    expect(again.side.width).toBe(DEFAULT_WIDTH.side);
    again.resetAll();
    expect(new Layout(store)).toMatchObject({
      side: { width: DEFAULT_WIDTH.side, collapsed: false },
      agents: { width: DEFAULT_WIDTH.agents, collapsed: false },
      swapped: false,
      groups: 0.5,
    });
  });

  it("starts from the defaults when the saved layout is broken or storage fails", () => {
    const broken = new Layout(memory({ "ovra.layout": "{not json" }));
    expect(broken.agents).toEqual({ width: DEFAULT_WIDTH.agents, collapsed: false });
    const odd = new Layout(memory({ "ovra.layout": '{"side":{"width":"wide"},"agents":{"width":5000}}' }));
    expect(odd.side.width).toBe(DEFAULT_WIDTH.side);
    odd.viewport = 1440;
    expect(odd.width("agents")).toBe(1440 - 52 - 16 - 320 - DEFAULT_WIDTH.side);

    const failing = memory();
    failing.setItem = () => {
      throw new Error("quota");
    };
    const layout = new Layout(failing);
    expect(() => layout.swap()).not.toThrow();
    expect(layout.swapped).toBe(true);
  });

  it("keeps the bottom dock's height within the main area and remembers it", () => {
    const store = memory();
    const layout = new Layout(store);
    expect(layout.dock).toBeNull();
    layout.resizeDock(300, 800);
    expect(new Layout(store).dock).toBe(300);
    // The files above keep 160 px, and the dock is never shorter than its minimum.
    layout.resizeDock(5000, 800);
    expect(layout.dock).toBe(640);
    layout.resizeDock(10, 800);
    expect(layout.dock).toBe(MIN_DOCK);
    layout.resizeDock(null, 800);
    expect(new Layout(store).dock).toBeNull();
    layout.resizeDock(300, 800);
    layout.resetAll();
    expect(new Layout(store).dock).toBeNull();
  });
});
