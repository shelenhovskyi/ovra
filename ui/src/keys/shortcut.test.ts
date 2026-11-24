import { describe, expect, it } from "vitest";
import { formatShortcut, matchesShortcut, parseShortcut } from "./shortcut";

const press = (key: string, code: string, mods: Partial<Record<"ctrl" | "meta" | "alt" | "shift", boolean>> = {}) => ({
  key,
  code,
  ctrlKey: mods.ctrl ?? false,
  metaKey: mods.meta ?? false,
  altKey: mods.alt ?? false,
  shiftKey: mods.shift ?? false,
});

describe("shortcuts", () => {
  it("maps Mod to Cmd on Apple keyboards and to Ctrl elsewhere", () => {
    expect(parseShortcut("Mod+Shift+P", true)).toEqual({ key: "p", ctrl: false, meta: true, alt: false, shift: true });
    expect(parseShortcut("Mod+Shift+P", false)).toEqual({ key: "p", ctrl: true, meta: false, alt: false, shift: true });
  });

  it("rejects unknown modifiers and empty keys", () => {
    expect(parseShortcut("Hyper+P", false)).toBeNull();
    expect(parseShortcut("Mod+", false)).toBeNull();
  });

  it("matches exact modifiers only", () => {
    const shortcut = parseShortcut("Mod+Shift+P", false);
    if (!shortcut) throw new Error("parse failed");
    expect(matchesShortcut(press("P", "KeyP", { ctrl: true, shift: true }), shortcut)).toBe(true);
    expect(matchesShortcut(press("p", "KeyP", { ctrl: true }), shortcut)).toBe(false);
    expect(matchesShortcut(press("P", "KeyP", { ctrl: true, shift: true, alt: true }), shortcut)).toBe(false);
  });

  it("falls back to the physical key on non-Latin layouts and named keys stay named", () => {
    const mod = parseShortcut("Mod+P", false);
    const f1 = parseShortcut("F1", false);
    if (!mod || !f1) throw new Error("parse failed");
    expect(matchesShortcut(press("\u0437", "KeyP", { ctrl: true }), mod)).toBe(true);
    expect(matchesShortcut(press("F1", "F1"), f1)).toBe(true);
  });

  it("formats labels per keyboard", () => {
    expect(formatShortcut("Mod+Shift+P", true)).toBe("⇧⌘P");
    expect(formatShortcut("Mod+Shift+P", false)).toBe("Ctrl+Shift+P");
    expect(formatShortcut("Alt+F1", false)).toBe("Alt+F1");
  });
});
