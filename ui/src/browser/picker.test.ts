import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeBrowserCore, fakePick } from "./fake-browser";
import { Picker } from "./picker.svelte";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

const flush = () => vi.advanceTimersByTimeAsync(0);

describe("Picker", () => {
  it("asks at most once per 100 ms while the pointer moves, then for the last point", async () => {
    const core = new FakeBrowserCore();
    const picker = new Picker(core, "pg1");
    picker.start();
    for (let x = 1; x <= 5; x++) picker.hover({ x, y: 1 });
    await flush();
    expect(core.sent("browser.pick")).toEqual([{ page: "pg1", x: 1, y: 1, up: 0 }]);
    expect(picker.hovered?.tag).toBe("button");
    await vi.advanceTimersByTimeAsync(100);
    expect(core.sent("browser.pick").map((p) => p.x)).toEqual([1, 5]);
    await vi.advanceTimersByTimeAsync(500);
    expect(core.sent("browser.pick")).toHaveLength(2);
  });

  it("moves up and down the ancestors, and not past the root or the element itself", async () => {
    const core = new FakeBrowserCore();
    const picker = new Picker(core, "pg1");
    picker.start();
    picker.child();
    picker.hover({ x: 4, y: 4 });
    await flush();
    picker.child();
    picker.parent();
    await flush();
    expect(picker.hovered?.tag).toBe("div1");
    picker.parent();
    await flush();
    expect([picker.up, picker.hovered?.tag]).toEqual([2, "div2"]);
    picker.child();
    await flush();
    expect(core.sent("browser.pick").map((p) => p.up)).toEqual([0, 1, 2, 1]);
    // Above the root the core answers the root again: Up stops there.
    core.element = () => fakePick("html", { x: 0, y: 0, width: 1, height: 1 });
    picker.parent();
    await flush();
    picker.parent();
    await flush();
    expect([picker.up, picker.hovered?.tag]).toEqual([2, "html"]);
  });

  it("takes the shown element for its point and asks for another point", async () => {
    const core = new FakeBrowserCore();
    const picker = new Picker(core, "pg1");
    picker.start();
    picker.hover({ x: 4, y: 4 });
    await flush();
    expect((await picker.take({ x: 4, y: 4 }))?.tag).toBe("button");
    expect(core.sent("browser.pick")).toHaveLength(1);
    await picker.take({ x: 9, y: 9 });
    expect(core.sent("browser.pick").at(-1)).toEqual({ page: "pg1", x: 9, y: 9, up: 0 });
  });

  it("asks nothing before it starts and drops late answers after it stops", async () => {
    const core = new FakeBrowserCore();
    const picker = new Picker(core, "pg1");
    picker.hover({ x: 1, y: 1 });
    expect(core.calls).toEqual([]);
    picker.start();
    picker.hover({ x: 1, y: 1 });
    picker.stop();
    await flush();
    expect([picker.active, picker.hovered]).toEqual([false, null]);
  });
});
