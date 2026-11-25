// Indentation as the editor preferences say: Tab, Shift+Tab, Mod+] / Mod+[
// and Enter. CodeMirror's own commands take the indent unit and the
// indentation of a new line from a language's syntax tree, which Ovra does
// not have (highlighting is tree-sitter; `@codemirror/language` is there for
// folding only), so these read `EditorState.tabSize` and `insertSpaces` here
// and keep a line's indentation as it is written.

import { countColumn, EditorSelection, EditorState, type Extension, Facet, type StateCommand } from "@codemirror/state";

/** Whether indenting inserts spaces (true) or tab characters. */
const insertSpaces = Facet.define<boolean, boolean>({ combine: (values) => values.at(-1) ?? true });

/** The facets for a tab size and indent style. */
export function indentation(tabSize: number, spaces: boolean): Extension {
  return [EditorState.tabSize.of(tabSize), insertSpaces.of(spaces)];
}

function unit(state: EditorState): string {
  return state.facet(insertSpaces) ? " ".repeat(state.tabSize) : "\t";
}

/** The start of every line a selection range touches, once each. */
function touchedLines(state: EditorState): number[] {
  const starts = new Set<number>();
  for (const range of state.selection.ranges) {
    // A selection that ends at a line's start leaves that line alone.
    const last = state.doc.lineAt(range.empty ? range.to : range.to - 1).number;
    for (let n = state.doc.lineAt(range.from).number; n <= last; n++) {
      starts.add(state.doc.line(n).from);
    }
  }
  return [...starts];
}

/** Indents every touched line by one unit. */
export const indentLines: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  const insert = unit(state);
  dispatch(state.update({ changes: touchedLines(state).map((from) => ({ from, insert })), userEvent: "input.indent" }));
  return true;
};

/** Takes one tab, or up to a tab size of spaces, from the start of every touched line. */
export const dedentLines: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  const changes = touchedLines(state).flatMap((from) => {
    const text = state.doc.lineAt(from).text;
    const spaces = /^ */.exec(text)?.[0].length ?? 0;
    const remove = text.startsWith("\t") ? 1 : Math.min(spaces, state.tabSize);
    return remove ? [{ from, to: from + remove }] : [];
  });
  if (changes.length) dispatch(state.update({ changes, userEvent: "delete.dedent" }));
  return true;
};

/** Tab: a selection indents its lines; a cursor gets a tab, or spaces to the next tab stop. */
export const indentTab: StateCommand = (target) => {
  const { state, dispatch } = target;
  if (state.readOnly) return false;
  if (state.selection.ranges.some((range) => !range.empty)) return indentLines(target);
  const spaces = state.facet(insertSpaces);
  dispatch(
    state.update(
      state.changeByRange((range) => {
        const line = state.doc.lineAt(range.head);
        const column = countColumn(line.text.slice(0, range.head - line.from), state.tabSize);
        const insert = spaces ? " ".repeat(state.tabSize - (column % state.tabSize)) : "\t";
        return { changes: { from: range.head, insert }, range: EditorSelection.cursor(range.head + insert.length) };
      }),
      { scrollIntoView: true, userEvent: "input.indent" },
    ),
  );
  return true;
};

/** Enter: a new line with the indentation of the line it breaks, as it is written (tabs stay tabs). */
export const newlineKeepIndent: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  dispatch(
    state.update(
      state.changeByRange((range) => {
        const line = state.doc.lineAt(range.from);
        const indent = /^[ \t]*/.exec(line.text.slice(0, range.from - line.from))?.[0] ?? "";
        const insert = `\n${indent}`;
        return {
          changes: { from: range.from, to: range.to, insert },
          range: EditorSelection.cursor(range.from + insert.length),
        };
      }),
      { scrollIntoView: true, userEvent: "input" },
    ),
  );
  return true;
};
