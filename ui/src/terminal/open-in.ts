// How other areas open a terminal in a folder (the file tree's "Open in
// terminal"): the terminal panel, once mounted, handles the request.

type Handler = (cwd: string) => void | Promise<void>;

let handler: Handler | null = null;

/** Opens a new terminal tab in the open project, its shell starting in the project-relative folder `cwd`. */
export function openTerminalIn(cwd: string): void {
  void handler?.(cwd);
}

/** Called by the terminal panel; returns a function that stops handling. */
export function handleOpenTerminalIn(next: Handler): () => void {
  handler = next;
  return () => {
    if (handler === next) handler = null;
  };
}
