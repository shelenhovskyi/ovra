// Breadcrumbs of the edited file: its path, and the symbols around the
// cursor (the language server's, else the outline read from the text).

import type { Text } from "@codemirror/state";
import { codeFoldAt, headingFoldAt, isMarkdown } from "./folding";
import { type FileSymbol, outlineSymbols } from "./symbols";

/** A path segment: its name and the project-relative path up to it. */
export type PathCrumb = { name: string; path: string };

export function pathCrumbs(path: string): PathCrumb[] {
  const parts = path.split("/").filter((part) => part !== "");
  return parts.map((name, i) => ({ name, path: parts.slice(0, i + 1).join("/") }));
}

/** The outline, each symbol's scope being what its line folds (its own line if nothing). */
export function scopedOutline(doc: Text, path: string, tabSize: number): FileSymbol[] {
  const markdown = isMarkdown(path);
  return outlineSymbols(doc, path).map((symbol) => {
    const line = doc.lineAt(symbol.from);
    const fold = markdown ? headingFoldAt(doc, line.number) : codeFoldAt(doc, line.number, tabSize);
    return { ...symbol, scope: { from: line.from, to: fold?.to ?? line.to } };
  });
}

/** The symbols whose scope holds `pos`, outermost first, each inside the one before. */
export function symbolTrail(symbols: readonly FileSymbol[], pos: number): FileSymbol[] {
  const holding = symbols.flatMap(({ scope, ...symbol }) =>
    scope && scope.from <= pos && pos <= scope.to ? [{ ...symbol, scope }] : [],
  );
  holding.sort((a, b) => a.scope.from - b.scope.from || b.scope.to - a.scope.to);
  const trail: typeof holding = [];
  for (const symbol of holding) {
    const last = trail.at(-1);
    if (!last || symbol.scope.to <= last.scope.to) trail.push(symbol);
  }
  return trail;
}
