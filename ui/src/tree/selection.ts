// Several selected rows in the tree: Mod+click toggles one, Shift+click and
// Shift+Arrow take the range from the anchor, in visible row order. Pure, so
// the tree only keeps the result.

import { isApple } from "../keys/shortcut";

/** The paths from `anchor` to `to`, both included, in the order the rows show. */
export function rangeOf(rows: { path: string }[], anchor: string, to: string): string[] {
  const a = rows.findIndex((r) => r.path === anchor);
  const b = rows.findIndex((r) => r.path === to);
  if (b < 0) return [];
  if (a < 0) return [to];
  const [from, end] = a <= b ? [a, b] : [b, a];
  return rows.slice(from, end + 1).map((r) => r.path);
}

/** `selection` with `path` added, or taken out when it is already in. */
export function toggled(selection: string[], path: string): string[] {
  return selection.includes(path) ? selection.filter((p) => p !== path) : [...selection, path];
}

/** The paths not inside another of them: moving or trashing a folder takes what is in it along. */
export function topLevel(paths: string[]): string[] {
  const unique = [...new Set(paths)];
  return unique.filter((p) => !unique.some((q) => q !== p && p.startsWith(`${q}/`)));
}

type Click = Pick<MouseEvent, "ctrlKey" | "metaKey" | "shiftKey">;

/** What a click with modifiers does to the selection. */
export function clickKind(e: Click, apple = isApple()): "toggle" | "range" | "single" {
  if (e.shiftKey) return "range";
  return (apple ? e.metaKey : e.ctrlKey) ? "toggle" : "single";
}
