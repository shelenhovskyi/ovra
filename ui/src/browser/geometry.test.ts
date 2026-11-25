import { describe, expect, it } from "vitest";
import { fitFrame, modifiers, toCanvas, toPage } from "./geometry";

const page = { width: 1280, height: 720, scroll_x: 0, scroll_y: 0 };

describe("geometry", () => {
  it("fits the page into the canvas, centred, without stretching", () => {
    // Half as wide as the page and much taller: bars above and below.
    expect(fitFrame({ width: 640, height: 600 }, page)).toEqual({ x: 0, y: 120, scale: 0.5, width: 640, height: 360 });
    // Wider than tall: bars left and right.
    expect(fitFrame({ width: 1000, height: 360 }, page)).toEqual({ x: 180, y: 0, scale: 0.5, width: 640, height: 360 });
  });

  it("maps canvas points to page points and back", () => {
    const frame = fitFrame({ width: 640, height: 600 }, page);
    expect(toPage({ x: 320, y: 300 }, frame)).toEqual({ x: 640, y: 360 });
    expect(toPage({ x: 0, y: 120 }, frame)).toEqual({ x: 0, y: 0 });
    // On the bars there is no page.
    expect(toPage({ x: 10, y: 50 }, frame)).toBeNull();
    expect(toCanvas({ x: 100, y: 40, width: 200, height: 80 }, frame)).toEqual({
      x: 50,
      y: 140,
      width: 100,
      height: 40,
    });
  });

  it("has no frame for an empty viewport", () => {
    expect(toPage({ x: 1, y: 1 }, fitFrame({ width: 100, height: 100 }, { ...page, width: 0 }))).toBeNull();
  });

  it("encodes modifiers as CDP does", () => {
    expect(modifiers({ altKey: true, ctrlKey: false, metaKey: false, shiftKey: true })).toBe(9);
    expect(modifiers({ altKey: false, ctrlKey: true, metaKey: true, shiftKey: false })).toBe(6);
  });
});
