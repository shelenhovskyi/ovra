// Command registry shared by the palette and keyboard shortcuts. Panels
// register their commands here; task 1.C builds the palette and key handling
// on top. Changing these types is a shared hot spot.

export type Command = {
  /** Stable id, `<area>.<name>`, e.g. `tree.reveal`. */
  id: string;
  title: string;
  /** Default shortcut, e.g. `Mod+P` (Mod is Cmd on macOS, Ctrl elsewhere). */
  keys?: string;
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
