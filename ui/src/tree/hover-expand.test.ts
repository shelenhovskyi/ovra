import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HOVER_EXPAND_MS, HoverExpander } from "./hover-expand";

describe("HoverExpander", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("opens a folder the drag rests on, not one it passes over or leaves", () => {
    const opened: string[] = [];
    const hover = new HoverExpander((path) => opened.push(path));

    hover.over("lib");
    vi.advanceTimersByTime(HOVER_EXPAND_MS - 1);
    hover.over("src"); // moved on before lib opened
    vi.advanceTimersByTime(HOVER_EXPAND_MS - 1);
    hover.over("src"); // dragover repeats while resting: the wait goes on
    vi.advanceTimersByTime(1);
    expect(opened).toEqual(["src"]);

    hover.over("docs");
    hover.over(null); // over a file or an open folder
    hover.over("tests");
    hover.cancel(); // dropped or left the tree
    vi.advanceTimersByTime(HOVER_EXPAND_MS * 2);
    expect(opened).toEqual(["src"]);
  });
});
