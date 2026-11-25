// Global keyboard shortcuts: a keydown on the window runs the registered
// command whose `keys` match.

import { type Command, listCommands } from "./commands";
import { inBrowserTab, isApple, liveKeys, matchesShortcut, parseShortcut } from "./shortcut";

type KeyEventLike = Parameters<typeof matchesShortcut>[0];

function findCommand(event: KeyEventLike, commands: Command[], apple: boolean, browser: boolean): Command | undefined {
  return commands.find((command) => {
    if (!command.keys) return false;
    const shortcut = parseShortcut(liveKeys(command.keys, browser), apple);
    return shortcut !== null && matchesShortcut(event, shortcut);
  });
}

/** Runs a command, reporting a failure instead of throwing into the caller's event handler. */
export async function runCommand(command: Command): Promise<void> {
  try {
    await command.run();
  } catch (error) {
    console.error(`command ${command.id} failed`, error);
  }
}

/** How long after a shortcut its menu bar item counts as the same press: the window may report both. */
const ECHO_MS = 500;
/** The command whose shortcut was pressed last, whether it ran here or a focused widget handled the keys. */
let pressed: { id: string; at: number } | undefined;

/** Runs the command picked in the menu bar, unless it is the echo of the shortcut just pressed. */
export function runMenuCommand(id: string): void {
  const echo = pressed?.id === id && Date.now() - pressed.at < ECHO_MS;
  pressed = undefined;
  if (echo) return;
  const command = listCommands().find((c) => c.id === id);
  if (command) void runCommand(command);
}

/**
 * Listens for shortcuts on `target`; returns a function that stops listening.
 * In a browser tab, shortcuts the browser keeps are replaced (see `liveKeys`).
 */
export function installKeybindings(target: Window, apple = isApple(), browser = inBrowserTab()): () => void {
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.isComposing) return;
    const command = findCommand(event, listCommands(), apple, browser);
    if (!command) return;
    pressed = { id: command.id, at: Date.now() };
    if (event.defaultPrevented) return;
    event.preventDefault();
    void runCommand(command);
  };
  // Bubble phase: a focused widget that handles the key itself (CodeMirror on
  // Mod-s) calls preventDefault first, and the command does not run twice.
  target.addEventListener("keydown", onKeydown);
  return () => target.removeEventListener("keydown", onKeydown);
}
