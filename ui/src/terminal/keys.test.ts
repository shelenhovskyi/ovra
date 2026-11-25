import { afterEach, describe, expect, it } from "vitest";
import { registerCommand } from "../keys/commands";
import { isCommandKey } from "./keys";

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
