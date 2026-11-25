// The one context menu of the window. A panel calls `openContextMenu` from its
// `contextmenu` event (or from a key, see `isMenuKey`); `ContextMenu.svelte`,
// mounted once by the shell, draws it. Opening another replaces it.

import { listCommands } from "../keys/commands";
import { runCommand } from "../keys/keybindings";

export type MenuItem = {
  label: string;
  /** A shortcut such as `Mod+C`, shown next to the label. */
  keys?: string;
  run: () => void | Promise<void>;
  disabled?: boolean;
  /** Something that deletes or cannot be undone. */
  danger?: boolean;
};

/** An item, or a line between groups of items. Falsy entries are left out, so `cond && item` works. */
export type MenuEntry = MenuItem | "separator" | null | false | undefined;

/** Where the menu opens: at the pointer of a mouse event, under the target of a key event, or at a point. */
export type MenuAnchor = MouseEvent | KeyboardEvent | { x: number; y: number; returnFocus?: HTMLElement | null };

type OpenMenu = { x: number; y: number; entries: (MenuItem | "separator")[]; returnFocus: HTMLElement | null };

export const contextMenu = $state<{ open: OpenMenu | null }>({ open: null });

/** Shift+F10 or the ContextMenu key: the keyboard's way to ask for a context menu. */
export function isMenuKey(event: KeyboardEvent): boolean {
  return event.code === "ContextMenu" || (event.code === "F10" && event.shiftKey && !event.ctrlKey && !event.metaKey);
}

function focused(): HTMLElement | null {
  const active = document.activeElement;
  return active instanceof HTMLElement && active !== document.body ? active : null;
}

/** The point an event asks for: the pointer, or under its target (near the top of a big one) when a key made it. */
function pointOf(event: MouseEvent | KeyboardEvent): { x: number; y: number } {
  // A keyboard-made `contextmenu` event may carry no position.
  if (event instanceof MouseEvent && (event.clientX !== 0 || event.clientY !== 0)) {
    return { x: event.clientX, y: event.clientY };
  }
  const box = event.target instanceof Element ? event.target.getBoundingClientRect() : null;
  return { x: box?.left ?? 0, y: box ? Math.min(box.bottom, box.top + 32) : 0 };
}

/**
 * Opens a menu of `entries` at `at`, replacing any open one. An event's native
 * menu is prevented. Focus goes back to `returnFocus` (by default whatever had
 * it) when the menu closes.
 */
export function openContextMenu(at: MenuAnchor, entries: MenuEntry[]): void {
  const items = tidy(entries);
  if (items.length === 0) return;
  let returnFocus = focused();
  let point: { x: number; y: number };
  if (at instanceof Event) {
    at.preventDefault();
    at.stopPropagation();
    point = pointOf(at);
  } else {
    point = { x: at.x, y: at.y };
    if (at.returnFocus !== undefined) returnFocus = at.returnFocus;
  }
  contextMenu.open = { ...point, entries: items, returnFocus };
}

/** Closes the menu; `refocus` gives focus back to where it was before the menu opened. */
export function closeContextMenu(refocus = true): void {
  const open = contextMenu.open;
  contextMenu.open = null;
  if (refocus) open?.returnFocus?.focus();
}

/** The registered command `id` as an item, with its shortcut; null while no such command is registered. */
export function commandItem(id: string, label?: string): MenuItem | null {
  const command = listCommands().find((c) => c.id === id);
  return command ? { label: label ?? command.title, keys: command.keys, run: () => runCommand(command) } : null;
}

/** Drops left-out entries, and separators at either end or doubled, so callers can leave out groups freely. */
function tidy(entries: MenuEntry[]): (MenuItem | "separator")[] {
  const out: (MenuItem | "separator")[] = [];
  for (const entry of entries) {
    if (!entry) continue;
    if (entry === "separator" && (out.length === 0 || out[out.length - 1] === "separator")) continue;
    out.push(entry);
  }
  if (out[out.length - 1] === "separator") out.pop();
  return out;
}

/** The top-left corner that keeps a `width` x `height` menu opened at `x`, `y` inside the viewport. */
export function clampToViewport(
  x: number,
  y: number,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
): { x: number; y: number } {
  const margin = 4;
  // Too low: open upwards from the point, as native menus do, when that fits.
  const top = y + size.height > viewport.height - margin && y - size.height >= margin ? y - size.height : y;
  return {
    x: Math.max(margin, Math.min(x, viewport.width - size.width - margin)),
    y: Math.max(margin, Math.min(top, viewport.height - size.height - margin)),
  };
}
