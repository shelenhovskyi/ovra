import { afterEach, describe, expect, it } from "vitest";
import { registerCommand } from "../keys/commands";
import { clipboardKey, findKeys, isCommandKey, isFindKey } from "./keys";

const press = (key: string, code: string, mods: Partial<Record<"ctrl" | "meta" | "alt" | "shift", boolean>> = {}) => ({
  key,
  code,
  ctrlKey: mods.ctrl ?? false,
  metaKey: mods.meta ?? false,
  altKey: mods.alt ?? false,
  shiftKey: mods.shift ?? false,
});

const offs: (() => void)[] = [];
afterEach(() => {
  for (const off of offs.splice(0)) off();
});

function command(id: string, keys: string) {
  offs.push(registerCommand({ id, title: id, keys, run: () => {} }));
}

describe("keys in a focused terminal", () => {
  it("leaves Ctrl+<letter> to the shell, also when a command uses it", () => {
    command("test.close", "Mod+W");
    expect(isCommandKey(press("w", "KeyW", { ctrl: true }), false, false)).toBe(false);
    expect(isCommandKey(press("c", "KeyC", { ctrl: true }), false, false)).toBe(false);
  });

  it("leaves Alt+arrows to the shell, which moves by word with them", () => {
    command("test.back", "Alt+ArrowLeft");
    command("test.forward", "Alt+ArrowRight");
    expect(isCommandKey(press("ArrowLeft", "ArrowLeft", { alt: true }), false, false)).toBe(false);
    expect(isCommandKey(press("ArrowRight", "ArrowRight", { alt: true }), false, false)).toBe(false);
  });

  it("runs commands on Cmd, Alt and Ctrl+Shift shortcuts", () => {
    command("test.close", "Mod+W");
    command("test.palette", "Mod+Shift+P");
    expect(isCommandKey(press("w", "KeyW", { meta: true }), true, false)).toBe(true);
    expect(isCommandKey(press("w", "KeyW", { alt: true }), false, true)).toBe(true);
    expect(isCommandKey(press("P", "KeyP", { ctrl: true, shift: true }), false, false)).toBe(true);
  });

  it("runs the terminal toggle on Ctrl+`, whatever the layout", () => {
    command("test.toggle", "Ctrl+`");
    expect(isCommandKey(press("`", "Backquote", { ctrl: true }), false, false)).toBe(true);
    expect(isCommandKey(press("\u0451", "Backquote", { ctrl: true }), true, true)).toBe(true);
    expect(isCommandKey(press("'", "Backquote", { ctrl: true }), false, true)).toBe(true);
  });
});

describe("isFindKey", () => {
  it("is Cmd+F on Apple keyboards and Ctrl+Shift+F elsewhere, matched by position", () => {
    expect(isFindKey(press("f", "KeyF", { meta: true }), true)).toBe(true);
    expect(isFindKey(press("φ", "KeyF", { ctrl: true, shift: true }), false)).toBe(true);
    // Ctrl+F stays the shell's.
    expect(isFindKey(press("f", "KeyF", { ctrl: true }), false)).toBe(false);
    expect(isFindKey(press("f", "KeyF", { ctrl: true }), true)).toBe(false);
    expect(isFindKey(press("F", "KeyF", { meta: true, shift: true }), true)).toBe(false);
    expect(isFindKey(press("g", "KeyG", { ctrl: true, shift: true }), false)).toBe(false);
    expect(findKeys(true)).toBe("Mod+F");
    expect(findKeys(false)).toBe("Ctrl+Shift+F");
  });
});

describe("clipboardKey", () => {
  it("copies and pastes with Shift off Apple keyboards, matched by position", () => {
    expect(clipboardKey(press("ψ", "KeyC", { ctrl: true, shift: true }), false)).toBe("copy");
    expect(clipboardKey(press("ω", "KeyV", { ctrl: true, shift: true }), false)).toBe("paste");
    expect(clipboardKey(press("Insert", "Insert", { ctrl: true }), false)).toBe("copy");
    expect(clipboardKey(press("Insert", "Insert", { shift: true }), false)).toBe("system-paste");
    // Ctrl+C and Ctrl+V stay the shell's.
    expect(clipboardKey(press("c", "KeyC", { ctrl: true }), false)).toBeNull();
    expect(clipboardKey(press("v", "KeyV", { ctrl: true }), false)).toBeNull();
  });

  it("copies with Cmd+C on Apple keyboards and leaves Cmd+V to the system", () => {
    expect(clipboardKey(press("c", "KeyC", { meta: true }), true)).toBe("copy");
    expect(clipboardKey(press("v", "KeyV", { meta: true }), true)).toBeNull();
    expect(clipboardKey(press("c", "KeyC", { ctrl: true }), true)).toBeNull();
  });
});
