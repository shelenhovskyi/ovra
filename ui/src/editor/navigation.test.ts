import { describe, expect, it } from "vitest";
import { JUMP_LINES, NavigationHistory, type Place } from "./navigation";

const at = (path: string, line?: number, column = 0): Place =>
  line === undefined ? { path, mode: "view" } : { path, mode: "edit", line, column };

describe("NavigationHistory", () => {
  it("adds a place for another file, and goes back and forward between them", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts"));
    history.visit(at("b.ts"));
    history.visit(at("c.ts"));
    expect(history.back()).toEqual(at("b.ts"));
    expect(history.back()).toEqual(at("a.ts"));
    expect(history.back()).toBeNull();
    expect(history.forward()).toEqual(at("b.ts"));
    expect(history.forward()).toEqual(at("c.ts"));
    expect(history.forward()).toBeNull();
  });

  it("follows small cursor moves and typing in the current place, and adds one for a far jump", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts", 1));
    history.visit(at("a.ts", 1 + JUMP_LINES, 4));
    expect(history.places).toEqual([at("a.ts", 1 + JUMP_LINES, 4)]);
    // A paste of many lines moves the cursor far without being a jump.
    history.visit(at("a.ts", 200, 2), true);
    expect(history.places).toEqual([at("a.ts", 200, 2)]);
    history.visit(at("a.ts", 20));
    expect(history.places).toEqual([at("a.ts", 200, 2), at("a.ts", 20)]);
    expect(history.back()).toEqual(at("a.ts", 200, 2));
  });

  it("keeps the cursor it knows when the file shows again without one, and the new mode", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts"));
    history.visit(at("a.ts", 30, 5));
    expect(history.places).toEqual([at("a.ts", 30, 5)]);
    history.visit({ path: "a.ts", mode: "view" });
    expect(history.places).toEqual([{ path: "a.ts", mode: "view", line: 30, column: 5 }]);
  });

  it("drops the places ahead on a new jump after going back", () => {
    const history = new NavigationHistory();
    for (const path of ["a.ts", "b.ts", "c.ts"]) history.visit(at(path));
    history.back();
    history.visit(at("b.ts"));
    history.visit(at("d.ts"));
    expect(history.places.map((p) => p.path)).toEqual(["a.ts", "b.ts", "d.ts"]);
    expect(history.forward()).toBeNull();
  });

  it("keeps the newest places up to its cap", () => {
    const history = new NavigationHistory(3);
    for (const path of ["a", "b", "c", "d", "e"]) history.visit(at(path));
    expect(history.places.map((p) => p.path)).toEqual(["c", "d", "e"]);
    expect(history.back()?.path).toBe("d");
    expect(history.back()?.path).toBe("c");
    expect(history.back()).toBeNull();
  });

  it("takes what the window shows while arriving at a place as that place, not as new jumps", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts", 50));
    history.visit(at("b.ts", 1));
    expect(history.back()).toEqual(at("a.ts", 50));
    // The file opens, its buffer shows the cursor it kept, then the spot is selected.
    history.visit({ path: "a.ts", mode: "edit" });
    history.visit(at("a.ts", 300));
    history.visit(at("a.ts", 50));
    expect(history.places).toEqual([at("a.ts", 50), at("b.ts", 1)]);
    // Arrived: a far jump counts again.
    history.visit(at("a.ts", 120));
    expect(history.places).toEqual([at("a.ts", 50), at("a.ts", 120)]);
  });

  it("goes back within one file", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts", 10));
    history.visit(at("a.ts", 400));
    expect(history.back()).toEqual(at("a.ts", 10));
    history.visit(at("a.ts", 10));
    expect(history.forward()).toEqual(at("a.ts", 400));
    history.visit(at("a.ts", 400));
    expect(history.places).toEqual([at("a.ts", 10), at("a.ts", 400)]);
  });

  it("stops arriving when another file shows first", () => {
    const history = new NavigationHistory();
    history.visit(at("a.ts", 50));
    history.visit(at("b.ts"));
    history.back();
    history.visit(at("c.ts"));
    expect(history.places.map((p) => p.path)).toEqual(["a.ts", "c.ts"]);
  });

  it("has nowhere to go while empty", () => {
    const history = new NavigationHistory();
    expect([history.back(), history.forward(), history.current]).toEqual([null, null, null]);
  });
});
