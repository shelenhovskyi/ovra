// Command registry shared by the palette and keyboard shortcuts. Panels
// register their commands here; task 1.C builds the palette and key handling
// on top. Changing these types is a shared hot spot.

export type Command = {
  /** Stable id, `<area>.<name>`, e.g. `tree.reveal`. */
  id: string;
  title: string;
  /** Default shortcut, e.g. `Mod+P` (Mod is Cmd on macOS, Ctrl elsewhere). */
  keys?: string;
  /**
   * The one place `keys` is meant for, such as `Editor`, as Settings > Keyboard
   * groups it (as `ContextKey.where`); unset for keys that work everywhere.
   */
  where?: string;
  /** Group shown beside the title in the palette (Git, View, Settings); else taken from the id's area. */
  category?: string;
  /** Other words the palette matches, e.g. `preferences` for Settings. */
  keywords?: string[];
  run: () => void | Promise<void>;
};

const commands = new Map<string, Command>();

/** Registers a command; returns a function that unregisters it. */
export function registerCommand(command: Command): () => void {
  commands.set(command.id, command);
  return () => commands.delete(command.id);
}

export function listCommands(): Command[] {
  return [...commands.values()].sort((a, b) => a.title.localeCompare(b.title));
}
