// Code folding. Highlighting is tree-sitter, so CodeMirror has no syntax
// tree to fold by: a line folds the lines indented deeper below it, or, when
// none are, up to the bracket that closes the one it ends with. Markdown
// folds by heading. The chevrons show while the pointer is on the gutter;
// a folded line keeps its chevron.

import { codeFolding, foldAll, foldCode, foldGutter, foldService, unfoldAll, unfoldCode } from "@codemirror/language";
import type { EditorState, Extension, Text } from "@codemirror/state";
import type { Command, KeyBinding } from "@codemirror/view";

/** A fold: the text from `from` to `to` hides behind a placeholder. */
export type FoldRange = { from: number; to: number };

/**
 * How far a fold looks below its line. Folds are asked for every line on
 * screen after each change, so a longer block simply does not fold.
 */
const MAX_LINES = 5000;

const OPENERS: Record<string, string> = { "{": "}", "[": "]", "(": ")" };

/** The indentation width of `text`, or -1 for a blank line. */
function indentWidth(text: string, tabSize: number): number {
  let width = 0;
  for (const char of text) {
    if (char === " ") width++;
    else if (char === "\t") width += tabSize - (width % tabSize);
    else return width;
  }
  return -1;
}

/** The lines below line `n` indented deeper than it, up to the last non-blank one. */
function indentFold(doc: Text, n: number, tabSize: number): FoldRange | null {
  const start = doc.line(n);
  const own = indentWidth(start.text, tabSize);
  if (own < 0) return null;
  let last = 0;
  for (let i = n + 1; i <= doc.lines; i++) {
    const width = indentWidth(doc.line(i).text, tabSize);
    if (width < 0) continue;
    if (width <= own) break;
    if (i - n > MAX_LINES) return null;
    last = i;
  }
  return last ? { from: start.to, to: doc.line(last).to } : null;
}

/** From after the bracket line `n` ends with to before the one that closes it (strings are not skipped). */
function bracketFold(doc: Text, n: number): FoldRange | null {
  const start = doc.line(n);
  const text = start.text.trimEnd();
  const open = text.at(-1) ?? "";
  const close = OPENERS[open];
  if (!close) return null;
  let depth = 1;
  const stop = Math.min(doc.lines, n + MAX_LINES);
  for (let i = n + 1; i <= stop; i++) {
    const line = doc.line(i);
    for (let at = 0; at < line.text.length; at++) {
      const char = line.text[at];
      if (char === open) depth++;
      else if (char === close && --depth === 0) {
        // A closing bracket alone on its line stays in sight, as with indentation.
        const to = line.text.slice(0, at).trim() === "" ? doc.line(i - 1).to : line.from + at;
        const from = start.from + text.length;
        return to > from ? { from, to } : null;
      }
    }
  }
  return null;
}

/** The fold that starts on line `n` of a program text, if any. */
export function codeFoldAt(doc: Text, n: number, tabSize: number): FoldRange | null {
  return indentFold(doc, n, tabSize) ?? bracketFold(doc, n);
}

/** The fold of a Markdown heading on line `n`: its section, up to the next heading at its level or above. */
export function headingFoldAt(doc: Text, n: number): FoldRange | null {
  const level = /^(#{1,6})\s/.exec(doc.line(n).text)?.[1]?.length;
  if (!level) return null;
  let fenced = false;
  let last = n;
  for (let i = n + 1; i <= doc.lines; i++) {
    const text = doc.line(i).text;
    if (/^\s*(```|~~~)/.test(text)) fenced = !fenced;
    const next = fenced ? undefined : /^(#{1,6})\s/.exec(text)?.[1]?.length;
    if (next !== undefined && next <= level) break;
    if (text.trim() !== "") last = i;
  }
  return last > n ? { from: doc.line(n).to, to: doc.line(last).to } : null;
}

/** The fold service, gutter and placeholder of a file; `markdown` folds by heading. */
export function folding(markdown: boolean): Extension {
  return [
    foldService.of((state: EditorState, lineStart: number) => {
      const n = state.doc.lineAt(lineStart).number;
      return markdown ? headingFoldAt(state.doc, n) : codeFoldAt(state.doc, n, state.tabSize);
    }),
    codeFolding({ placeholderText: "…" }),
    foldGutter({
      markerDOM: (open) => {
        const marker = document.createElement("span");
        marker.className = open ? "cm-fold-open" : "cm-fold-closed";
        marker.textContent = open ? "⌄" : "›";
        marker.title = open ? "Fold" : "Unfold";
        return marker;
      },
    }),
  ];
}

/**
 * Fold and unfold at the cursor, with the keys VS Code uses: Cmd+Alt+[ / ]
 * on macOS, Ctrl+Shift+[ / ] elsewhere, where Ctrl+Alt is AltGr and types
 * brackets on many layouts. Fold all has no key (Mod+K starts VS Code's).
 */
export const foldKeys: KeyBinding[] = [
  { key: "Ctrl-Shift-[", mac: "Cmd-Alt-[", run: foldCode },
  { key: "Ctrl-Shift-]", mac: "Cmd-Alt-]", run: unfoldCode },
];

export const foldCommands: Record<"fold" | "unfold" | "fold_all" | "unfold_all", Command> = {
  fold: foldCode,
  unfold: unfoldCode,
  fold_all: foldAll,
  unfold_all: unfoldAll,
};
