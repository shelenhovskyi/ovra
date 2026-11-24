// Global keyboard shortcuts: a keydown on the window runs the registered
// command whose `keys` match.

import { type Command, listCommands } from "./commands";
import { isApple, matchesShortcut, parseShortcut } from "./shortcut";

type KeyEventLike = Parameters<typeof matchesShortcut>[0];

function findCommand(event: KeyEventLike, commands: Command[], apple: boolean): Command | undefined {
  return commands.find((command) => {
    if (!command.keys) return false;
    const shortcut = parseShortcut(command.keys, apple);
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

/** Listens for shortcuts on `target`; returns a function that stops listening. */
export function installKeybindings(target: Window, apple = isApple()): () => void {
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.defaultPrevented || event.isComposing) return;
    const command = findCommand(event, listCommands(), apple);
    if (!command) return;
    event.preventDefault();
    void runCommand(command);
  };
  // Bubble phase: a focused widget that handles the key itself (CodeMirror on
  // Mod-s) calls preventDefault first, and the command does not run twice.
  target.addEventListener("keydown", onKeydown);
  return () => target.removeEventListener("keydown", onKeydown);
}
