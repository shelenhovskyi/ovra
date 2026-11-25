import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { registerCommand } from "../keys/commands";
import ContextMenu from "./ContextMenu.svelte";
import { clampToViewport, closeContextMenu, commandItem, isMenuKey, openContextMenu } from "./context-menu.svelte";

let app: Record<string, unknown>;
let origin: HTMLButtonElement;

beforeEach(() => {
  origin = document.body.appendChild(document.createElement("button"));
  app = mount(ContextMenu, { target: document.body });
});

afterEach(() => {
  closeContextMenu(false);
  unmount(app);
  document.body.replaceChildren();
});

const items = () => [...document.querySelectorAll<HTMLButtonElement>("[role=menuitem]")];
const press = (code: string) =>
  document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true }));

describe("ContextMenu", () => {
  it("shows items between tidy separators, moves with keys, runs one and gives focus back", () => {
    const copy = vi.fn();
    const paste = vi.fn();
    openContextMenu({ x: 10, y: 20, returnFocus: origin }, [
      "separator",
      { label: "Copy", keys: "Ctrl+C", run: copy },
      "separator",
      false,
      commandItem("test.missing"),
      "separator",
      { label: "Paste", run: paste, disabled: true },
      { label: "Select all", run: vi.fn() },
      "separator",
    ]);
    flushSync();
    expect(document.querySelector("[role=menu]")?.querySelectorAll("[role=separator]")).toHaveLength(1);
    expect(items().map((item) => item.textContent?.trim())).toEqual(["Copy Ctrl+C", "Paste", "Select all"]);
    expect(document.activeElement).toBe(items()[0]);
    press("ArrowDown");
    expect(document.activeElement).toBe(items()[1]);
    press("KeyS");
    expect(document.activeElement).toBe(items()[2]);

    items()[1]?.click();
    expect(paste).not.toHaveBeenCalled();
    items()[0]?.click();
    flushSync();
    expect(copy).toHaveBeenCalledOnce();
    expect(document.querySelector("[role=menu]")).toBeNull();
    expect(document.activeElement).toBe(origin);
  });

  it("closes on Escape or a press outside, giving focus back", () => {
    openContextMenu({ x: 0, y: 0, returnFocus: origin }, [{ label: "Copy", run: vi.fn() }]);
    flushSync();
    press("Escape");
    flushSync();
    expect(document.querySelector("[role=menu]")).toBeNull();
    expect(document.activeElement).toBe(origin);

    openContextMenu({ x: 0, y: 0, returnFocus: origin }, [{ label: "Copy", run: vi.fn() }]);
    flushSync();
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    flushSync();
    expect(document.querySelector("[role=menu]")).toBeNull();
    expect(document.activeElement).toBe(origin);
  });
});

describe("menu helpers", () => {
  it("makes an item of a registered command, and knows the menu keys", async () => {
    const run = vi.fn();
    const off = registerCommand({ id: "test.find", title: "Find in file", keys: "Mod+F", run });
    const item = commandItem("test.find", "Find");
    off();
    expect(item).toMatchObject({ label: "Find", keys: "Mod+F" });
    await item?.run();
    expect(run).toHaveBeenCalledOnce();
    expect(isMenuKey(new KeyboardEvent("keydown", { code: "F10", shiftKey: true }))).toBe(true);
    expect(isMenuKey(new KeyboardEvent("keydown", { code: "ContextMenu" }))).toBe(true);
    expect(isMenuKey(new KeyboardEvent("keydown", { code: "F10" }))).toBe(false);
  });
});

describe("clampToViewport", () => {
  const viewport = { width: 800, height: 600 };
  const size = { width: 200, height: 100 };
  it("keeps the point when the menu fits, and stays inside the window otherwise", () => {
    expect(clampToViewport(10, 20, size, viewport)).toEqual({ x: 10, y: 20 });
    expect(clampToViewport(790, 20, size, viewport)).toEqual({ x: 596, y: 20 });
    // Near the bottom it opens upwards from the point.
    expect(clampToViewport(10, 580, size, viewport)).toEqual({ x: 10, y: 480 });
    expect(clampToViewport(10, 50, { width: 200, height: 700 }, viewport)).toEqual({ x: 10, y: 4 });
  });
});
