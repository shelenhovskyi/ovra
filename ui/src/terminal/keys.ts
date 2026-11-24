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
