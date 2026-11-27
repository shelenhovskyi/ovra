// Indentation as the editor preferences say: Tab, Shift+Tab, Mod+] / Mod+[
// and Enter. CodeMirror's own commands take the indent unit and the
// indentation of a new line from a language's syntax tree, which Ovra does
// not have (highlighting is tree-sitter; `@codemirror/language` is there for
// folding and bracket scanning only), so these read `EditorState.tabSize` and
// `insertSpaces` here, keep a line's indentation as it is written, and go one
// level deeper after an opening bracket (or Python's colon).

import { matchBrackets } from "@codemirror/language";
import { countColumn, EditorSelection, EditorState, type Extension, Facet, type StateCommand } from "@codemirror/state";

/** Whether indenting inserts spaces (true) or tab characters. */
const insertSpaces = Facet.define<boolean, boolean>({ combine: (values) => values.at(-1) ?? true });

/** The facets for a tab size and indent style. */
export function indentation(tabSize: number, spaces: boolean): Extension {
  return [EditorState.tabSize.of(tabSize), insertSpaces.of(spaces)];
}

/**
 * The facets for a file: its own indentation when it has one (`detected`,
 * "Tab" or "Spaces: N" from `detectIndent`), else the preferences.
 */
export function fileIndentation(detected: string | null, tabSize: number, spaces: boolean): Extension {
  if (detected === "Tab") return indentation(tabSize, false);
  const width = Number(/^Spaces: (\d+)$/.exec(detected ?? "")?.[1]);
  return width > 0 ? indentation(width, true) : indentation(tabSize, spaces);
}

/** What Tab inserts, as the status bar says it: "Tab" or "Spaces: N". */
export function indentLabel(state: EditorState): string {
  return state.facet(insertSpaces) ? `Spaces: ${state.tabSize}` : "Tab";
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

/** Text at the end of a line that makes the next line one level deeper, by language id. */
const OPENS_BLOCK: Record<string, RegExp> = { python: /[:([{]$/ };
const OPENS_BRACKET = /[([{]$/;

/** Which line endings open a block: brackets everywhere, and a colon in Python. */
const blockOpener = Facet.define<RegExp, RegExp>({ combine: (values) => values.at(-1) ?? OPENS_BRACKET });

/** Indenting as `languageId` writes blocks (Enter after `{`, Python's `:`), and closing brackets that find their place. */
export function smartIndent(languageId: string | null): Extension {
  return [blockOpener.of((languageId && OPENS_BLOCK[languageId]) || OPENS_BRACKET), closerFindsIndent];
}

const CLOSER: Record<string, string> = { "(": ")", "[": "]", "{": "}" };

/**
 * Enter: a new line with the indentation of the line it breaks, as it is
 * written (tabs stay tabs), one level deeper after an opening bracket. Between
 * a bracket pair (`{|}`) the closing one moves to a line of its own.
 */
export const newlineKeepIndent: StateCommand = ({ state, dispatch }) => {
  if (state.readOnly) return false;
  const opener = state.facet(blockOpener);
  dispatch(
    state.update(
      state.changeByRange((range) => {
        const line = state.doc.lineAt(range.from);
        const before = line.text.slice(0, range.from - line.from);
        const indent = /^[ \t]*/.exec(before)?.[0] ?? "";
        const head = before.trimEnd();
        const deeper = opener.test(head);
        // Spaces right after the cursor would only lead the new line.
        const after = /^[ \t]*/.exec(line.text.slice(range.to - line.from))?.[0].length ?? 0;
        const to = range.to + after;
        const next = line.text.charAt(to - line.from);
        const first = `\n${deeper ? indent + unit(state) : indent}`;
        const split = deeper && next !== "" && next === CLOSER[head.at(-1) ?? ""];
        const insert = split ? `${first}\n${indent}` : first;
        // Spaces before the cursor would be left at the end of the line.
        const from = head ? line.from + head.length : range.from;
        return {
          changes: { from, to, insert },
          range: EditorSelection.cursor(from + first.length),
        };
      }),
      { scrollIntoView: true, userEvent: "input" },
    ),
  );
  return true;
};

/**
 * A closing bracket typed first on its line takes the indentation of the
 * line that opens its pair, as `}` does after a block.
 */
const closerFindsIndent = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || !tr.isUserEvent("input.type") || tr.newSelection.ranges.length > 1) return tr;
  const head = tr.newSelection.main.head;
  const typed = tr.newDoc.sliceString(head - 1, head);
  if (!")]}".includes(typed) || typed === "") return tr;
  const line = tr.newDoc.lineAt(head);
  const lead = line.text.slice(0, head - 1 - line.from);
  if (!/^[ \t]*$/.test(lead)) return tr;
  const state = tr.state;
  const match = matchBrackets(state, head, -1);
  if (!match?.matched || !match.end) return tr;
  const openLine = state.doc.lineAt(match.end.from);
  const indent = /^[ \t]*/.exec(openLine.text)?.[0] ?? "";
  if (openLine.number === line.number || indent === lead) return tr;
  return [tr, { changes: { from: line.from, to: line.from + lead.length, insert: indent }, sequential: true }];
});
