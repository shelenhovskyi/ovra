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
  if (command && allowed(command, document)) void runCommand(command);
}

/**
 * Whether `command` may run now. While a modal dialog is open (the palette,
 * the close question) only the commands it names in `data-commands` (an id
 * prefix such as `palette.`) do; the rest would act behind it.
 */
function allowed(command: Command, doc: Document): boolean {
  const modal = doc.querySelector<HTMLElement>("[aria-modal='true']");
  if (!modal) return true;
  const own = modal.dataset.commands;
  return own !== undefined && command.id.startsWith(own);
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
    // Also when it does not run here: the menu bar's echo of the key must not run it either.
    pressed = { id: command.id, at: Date.now() };
    if (event.defaultPrevented) return;
    // Behind a modal the key is not the browser's either (Mod+S would save the page).
    event.preventDefault();
    if (allowed(command, target.document)) void runCommand(command);
  };
  // Bubble phase: a focused widget that handles the key itself (CodeMirror on
  // Mod-s) calls preventDefault first, and the command does not run twice.
  target.addEventListener("keydown", onKeydown);
  return () => target.removeEventListener("keydown", onKeydown);
}
