// A key the editor binds is not seen by the global shortcuts (a binding that
// runs calls preventDefault, and a chord's first press waits for the second),
// so no editor binding may start with a global shortcut.

import {
  findReferencesKeymap,
  formatKeymap,
  jumpToDefinitionKeymap,
  renameKeymap,
  signatureKeymap,
} from "@codemirror/lsp-client";
import { EditorState } from "@codemirror/state";
import { EditorView, type KeyBinding, keymap } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { liveKeys, parseShortcut, type Shortcut } from "../keys/shortcut";
import { editorKeymap } from "./buffers.svelte";
import { actionKeys } from "./lsp/actions";

const sources = import.meta.glob<string>(["../**/*.ts", "../**/*.svelte", "!../**/*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** The editor's own commands, which its keymap runs itself: the same action either way. */
const SAME_ACTION = new Set([
  "Mod+S",
  "Mod+F",
  "Mod+Alt+F",
  "Ctrl+G",
  "Shift+F12",
  "Shift+Alt+F",
  "Mod+Shift+M",
  "Mod+/",
]);

/** Every `keys` given to `registerCommand`, as a literal or a constant of the same file. */
function globalShortcuts(): { id: string; keys: string }[] {
  const found: { id: string; keys: string }[] = [];
  for (const text of Object.values(sources)) {
    for (const rest of text.split("registerCommand(").slice(1)) {
      // Only the command object, which names its keys before `run`; a menu's keys further down are not global.
      const call = rest.slice(0, rest.search(/\brun\s*[:,]/));
      const id = /id: [`"]([^`"]+)[`"]/.exec(call)?.[1] ?? "?";
      const value = /keys: (?:"([^"]+)"|([A-Z_]+)\b)/.exec(call);
      const name = value?.[2];
      const keys = value?.[1] ?? (name && new RegExp(`const ${name} = "([^"]+)"`).exec(text)?.[1]);
      if (keys && !SAME_ACTION.has(keys)) found.push({ id, keys });
    }
  }
  return found;
}

/** A CodeMirror key name such as `Mod-Shift-m` or `Alt-A`, as one press. */
function cmPress(name: string, apple: boolean, shift = false): Shortcut {
  const parts = name.split(/-(?!$)/);
  const last = parts.pop() ?? "";
  const press: Shortcut = { key: "", ctrl: false, meta: false, alt: false, shift };
  for (const part of parts) {
    if (/^(mod)$/i.test(part)) press[apple ? "meta" : "ctrl"] = true;
    else if (/^(cmd|meta|m)$/i.test(part)) press.meta = true;
    else if (/^(ctrl|control|c)$/i.test(part)) press.ctrl = true;
    else if (/^(alt|a)$/i.test(part)) press.alt = true;
    else if (/^(shift|s)$/i.test(part)) press.shift = true;
  }
  const key = last === "Space" ? " " : last;
  if (key.length === 1 && key !== key.toLowerCase()) press.shift = true;
  press.key = key.toLowerCase();
  return press;
}

/** The first press of each binding on one platform, with its Shift variant. */
function firstPresses(binding: KeyBinding, platform: "mac" | "linux" | "win"): Shortcut[] {
  const name = binding[platform] ?? binding.key;
  if (!name) return [];
  const first = name.split(" ")[0] ?? "";
  const apple = platform === "mac";
  return [cmPress(first, apple), ...(binding.shift ? [cmPress(first, apple, true)] : [])];
}

const same = (a: Shortcut, b: Shortcut) =>
  a.key === b.key && a.ctrl === b.ctrl && a.meta === b.meta && a.alt === b.alt && a.shift === b.shift;

function editorBindings(): readonly KeyBinding[] {
  const context = { note: () => {}, serverName: () => "", references: () => false };
  return [
    ...editorKeymap(() => {}),
    ...actionKeys(context),
    ...formatKeymap,
    ...renameKeymap,
    ...jumpToDefinitionKeymap,
    ...findReferencesKeymap,
    ...signatureKeymap,
  ];
}

describe("editor keymaps", () => {
  it("finds the global shortcuts", () => {
    const keys = globalShortcuts().map((g) => g.keys);
    expect(keys).toEqual(expect.arrayContaining(["Mod+K", "Mod+Shift+A", "Ctrl+`", "Mod+Alt+S"]));
    // Keys of a context menu after a command without keys are not global.
    expect(keys).not.toContain("Delete");
  });

  it("never start with a global shortcut", () => {
    const clashes: string[] = [];
    for (const platform of ["mac", "linux", "win"] as const) {
      for (const browser of [false, true]) {
        for (const global of globalShortcuts()) {
          const live = liveKeys(global.keys, browser);
          const shortcut = parseShortcut(live.split(" ")[0] ?? "", platform === "mac");
          if (!shortcut) continue;
          for (const binding of editorBindings()) {
            if (firstPresses(binding, platform).some((press) => same(press, shortcut)))
              clashes.push(`${binding[platform] ?? binding.key} takes ${live} (${global.id}) on ${platform}`);
          }
        }
      }
    }
    expect([...new Set(clashes)]).toEqual([]);
  });

  it("save on exactly Mod+S, leaving Mod+Shift+S to Save all", () => {
    const save = vi.fn();
    const view = new EditorView({
      state: EditorState.create({ extensions: keymap.of(editorKeymap(save)) }),
      parent: document.body,
    });
    const press = (init: KeyboardEventInit) => {
      const event = new KeyboardEvent("keydown", { keyCode: 83, code: "KeyS", cancelable: true, ...init });
      view.contentDOM.dispatchEvent(event);
      return event.defaultPrevented;
    };
    // CodeMirror alone reads Shift with a lower-case key as Mod-s.
    expect(press({ key: "s", ctrlKey: true, shiftKey: true })).toBe(false);
    expect(press({ key: "S", ctrlKey: true, shiftKey: true })).toBe(false);
    expect(save).not.toHaveBeenCalled();
    expect(press({ key: "s", ctrlKey: true })).toBe(true);
    // A Ukrainian layout: the physical S key still saves.
    expect(press({ key: String.fromCharCode(0x456), ctrlKey: true })).toBe(true);
    expect(save).toHaveBeenCalledTimes(2);
    // Replace's other key opens the panel at its replace field.
    expect(press({ key: "f", code: "KeyF", keyCode: 70, ctrlKey: true, altKey: true })).toBe(true);
    expect(document.activeElement?.getAttribute("name")).toBe("replace");
    view.destroy();
  });
});
