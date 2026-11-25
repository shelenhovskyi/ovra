import { test as base, expect } from "@playwright/test";

// The desktop window runs with Tauri's `freezePrototype` (app/tauri.conf.json),
// so every page here does too: code that assigns a property Object.prototype
// already has (`obj.toString = f`) throws there and blanks the window, while a
// plain browser lets it pass. Uncaught page errors fail the test for the same
// reason: in the window nothing else would show them.
export const test = base.extend({
  context: async ({ context }, use) => {
    await context.addInitScript(() => Object.freeze(Object.prototype));
    await use(context);
  },
  page: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await use(page);
    expect(errors, "uncaught errors in the page").toEqual([]);
  },
});

export { expect };

/** A shortcut as the UI labels it on this OS: `⌥P` on a Mac, `Alt+P` elsewhere. */
export function shortcutLabel(text: string): string {
  if (process.platform !== "darwin") return text;
  const parts = text.split("+");
  const key = parts.pop() ?? "";
  const symbols: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" };
  return (
    ["Ctrl", "Alt", "Shift", "Meta"]
      .filter((m) => parts.includes(m))
      .map((m) => symbols[m])
      .join("") + key
  );
}
