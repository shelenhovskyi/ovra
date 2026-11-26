// Which keys a focused terminal leaves to Ovra's commands. The shell needs
// most keys, including Ctrl+<letter> (Ctrl+C, Ctrl+R, Ctrl+W, ...), so only
// shortcuts the shell has no use for reach the command registry.

import { listCommands } from "../keys/commands";
import { liveKeys, matchesShortcut, parseShortcut, type Shortcut } from "../keys/shortcut";

type KeyEventLike = Parameters<typeof matchesShortcut>[0];

/** Ctrl+<letter> and Ctrl+<punctuation> without Shift or Alt are shell keys on every platform. */
function shellOwns(shortcut: Shortcut): boolean {
  return shortcut.ctrl && !shortcut.shift && !shortcut.alt && !shortcut.meta && shortcut.key !== "`";
}

/** Whether a keydown in the terminal should run an Ovra command instead of going to the shell. */
export function isCommandKey(event: KeyEventLike, apple: boolean, browser: boolean): boolean {
  return listCommands().some((command) => {
    if (!command.keys) return false;
    const shortcut = parseShortcut(liveKeys(command.keys, browser), apple);
    return shortcut !== null && !shellOwns(shortcut) && matchesShortcut(event, shortcut);
  });
}

/**
 * The terminal's own copy and paste keys. Plain Ctrl+C and Ctrl+V belong to
 * the shell off Apple keyboards, so there it is Ctrl+Shift+C and Ctrl+Shift+V
 * (and Ctrl+Insert, Shift+Insert), as in other terminals. On Apple keyboards
 * Cmd+C copies; Cmd+V is the system's paste, which xterm.js already takes.
 * Shift+Insert is the system's paste too: `system-paste` means the key is
 * kept from the shell but not prevented, so the browser pastes and xterm.js
 * takes the paste event, which works where reading the clipboard is denied.
 * Matched on `code`, so the keys work on any layout.
 */
export function clipboardKey(
  event: Pick<KeyboardEvent, "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">,
  apple: boolean,
): "copy" | "paste" | "system-paste" | null {
  const { code, ctrlKey: ctrl, shiftKey: shift, altKey: alt, metaKey: meta } = event;
  if (alt) return null;
  if (apple) return meta && !ctrl && !shift && code === "KeyC" ? "copy" : null;
  if (meta) return null;
  if (ctrl && shift && code === "KeyC") return "copy";
  if (ctrl && shift && code === "KeyV") return "paste";
  if (ctrl && !shift && code === "Insert") return "copy";
  if (shift && !ctrl && code === "Insert") return "system-paste";
  return null;
}

/** How the menu shows the copy and paste keys. */
export function clipboardKeys(apple: boolean): { copy: string; paste: string } {
  return apple ? { copy: "Mod+C", paste: "Mod+V" } : { copy: "Ctrl+Shift+C", paste: "Ctrl+Shift+V" };
}
