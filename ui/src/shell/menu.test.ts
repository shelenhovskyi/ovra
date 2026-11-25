import { afterEach, describe, expect, it } from "vitest";
import { menuTarget } from "./menu";

function items(...names: string[]): HTMLElement[] {
  return names.map((name) => {
    const button = document.body.appendChild(document.createElement("button"));
    button.setAttribute("aria-label", name);
    return button;
  });
}

const press = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

afterEach(() => {
  document.body.replaceChildren();
});

describe("menuTarget", () => {
  it("moves with arrows, Home and End, wrapping around", () => {
    const all = items("Audit log", "Servers", "Agent browser");
    all[0]?.focus();
    expect(menuTarget(press({ code: "ArrowUp" }), all)).toBe(all[2]);
    expect(menuTarget(press({ code: "ArrowDown" }), all)).toBe(all[1]);
    expect(menuTarget(press({ code: "End" }), all)).toBe(all[2]);
    expect(menuTarget(press({ code: "Home" }), all)).toBe(all[0]);
    expect(menuTarget(press({ code: "Escape" }), all)).toBeNull();
  });

  it("jumps to the next item starting with a typed letter, cycling through matches", () => {
    const all = items("Audit log", "Project policies", "Network and secrets", "Agent browser");
    all[0]?.focus();
    expect(menuTarget(press({ key: "a", code: "KeyA" }), all)).toBe(all[3]);
    all[3]?.focus();
    expect(menuTarget(press({ key: "A", code: "KeyA", shiftKey: true }), all)).toBe(all[0]);
    expect(menuTarget(press({ key: "z", code: "KeyZ" }), all)).toBeNull();
  });

  it("finds by the physical key on a non-Latin layout, and ignores chords", () => {
    const all = items("Audit log", "Project policies");
    all[0]?.focus();
    expect(menuTarget(press({ key: "\u0437", code: "KeyP" }), all)).toBe(all[1]);
    expect(menuTarget(press({ key: "p", code: "KeyP", ctrlKey: true }), all)).toBeNull();
  });
});
