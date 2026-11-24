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

/**
 * Listens for shortcuts on `target`; returns a function that stops listening.
 * In a browser tab, shortcuts the browser keeps are replaced (see `liveKeys`).
 */
export function installKeybindings(target: Window, apple = isApple(), browser = inBrowserTab()): () => void {
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.defaultPrevented || event.isComposing) return;
    const command = findCommand(event, listCommands(), apple, browser);
    if (!command) return;
    event.preventDefault();
    void runCommand(command);
  };
  // Bubble phase: a focused widget that handles the key itself (CodeMirror on
  // Mod-s) calls preventDefault first, and the command does not run twice.
  target.addEventListener("keydown", onKeydown);
  return () => target.removeEventListener("keydown", onKeydown);
}
