import { describe, expect, it } from "vitest";
import { RENDER_ALL_BELOW, visibleRange } from "./lines";

describe("visibleRange", () => {
  it("renders short lists in full", () => {
    expect(visibleRange(RENDER_ALL_BELOW, 5000, 100, 18)).toEqual({ start: 0, end: RENDER_ALL_BELOW });
  });

  it("windows long lists around the viewport with overscan", () => {
    expect(visibleRange(30_000, 0, 180, 18, 5)).toEqual({ start: 0, end: 15 });
    expect(visibleRange(30_000, 1800, 180, 18, 5)).toEqual({ start: 95, end: 115 });
    expect(visibleRange(30_000, 18 * 30_000, 180, 18, 5)).toEqual({ start: 29_995, end: 30_000 });
  });

  it("still shows rows before the viewport has a height", () => {
    expect(visibleRange(30_000, 0, 0, 18, 5)).toEqual({ start: 0, end: 6 });
  });
});
