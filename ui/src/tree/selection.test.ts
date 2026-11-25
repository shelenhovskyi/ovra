import { describe, expect, it } from "vitest";
import { clickKind, rangeOf, toggled, topLevel } from "./selection";

const rows = ["src", "src/a.ts", "src/b.ts", "README.md"].map((path) => ({ path }));

describe("selection", () => {
  it("takes a range in row order, either way, from a missing anchor only the row", () => {
    expect(rangeOf(rows, "src/a.ts", "README.md")).toEqual(["src/a.ts", "src/b.ts", "README.md"]);
    expect(rangeOf(rows, "src/b.ts", "src")).toEqual(["src", "src/a.ts", "src/b.ts"]);
    expect(rangeOf(rows, "gone", "src")).toEqual(["src"]);
  });

  it("toggles a path and keeps only the outermost of nested ones", () => {
    expect(toggled(["a"], "b")).toEqual(["a", "b"]);
    expect(toggled(["a", "b"], "a")).toEqual(["b"]);
    expect(topLevel(["src/a.ts", "src", "srcx", "src"])).toEqual(["src", "srcx"]);
  });

  it("reads Mod as Cmd on Apple keyboards and Ctrl elsewhere", () => {
    const click = (mods: Partial<MouseEvent>) => ({ ctrlKey: false, metaKey: false, shiftKey: false, ...mods });
    expect(clickKind(click({ metaKey: true }), true)).toBe("toggle");
    expect(clickKind(click({ ctrlKey: true }), true)).toBe("single");
    expect(clickKind(click({ ctrlKey: true }), false)).toBe("toggle");
    expect(clickKind(click({ shiftKey: true }), false)).toBe("range");
  });
});
