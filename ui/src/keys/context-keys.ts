// Keys that work only in one place (Y on an approval card, S on a Git row,
// Ctrl+N in the palette). Panels declare them here so Settings > Keyboard
// lists every key in one sheet; they are shown only, never bound globally:
// the panel that declares a key also handles it.

export type ContextKey = {
  /** As in `Command.keys`: `Shift+F10`, `Mod+Enter`, or a single key such as `Y`. */
  keys: string;
  title: string;
  /** Where it works, the group it is listed under: Approvals, Git, Files. */
  where: string;
};

const declared = new Set<ContextKey[]>();

/** Declares `keys`; returns a function that takes them back. */
export function registerContextKeys(keys: ContextKey[]): () => void {
  declared.add(keys);
  return () => declared.delete(keys);
}

/** Every declared key, in the order each place declared them. */
export function listContextKeys(): ContextKey[] {
  return [...declared].flat();
}
