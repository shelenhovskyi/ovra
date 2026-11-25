import { EditorSelection, EditorState, type StateCommand } from "@codemirror/state";
import { expect, test } from "vitest";
import { dedentLines, indentation, indentTab, newlineKeepIndent } from "./indent";

/** Runs `command` on `doc` with the cursor at `|` (or a selection between two), returning the text with `|` at the cursor. */
function run(command: StateCommand, doc: string, tabSize: number, spaces: boolean): string {
  const marks = [...doc.matchAll(/\|/g)].map((m, i) => (m.index ?? 0) - i);
  let state = EditorState.create({
    doc: doc.replaceAll("|", ""),
    selection: EditorSelection.single(marks[0] ?? 0, marks[1] ?? marks[0] ?? 0),
    extensions: indentation(tabSize, spaces),
  });
  command({ state, dispatch: (tr) => (state = tr.state) });
  const head = state.selection.main.head;
  const text = state.doc.toString();
  return `${text.slice(0, head)}|${text.slice(head)}`;
}

test("Tab inserts spaces to the next tab stop, or a tab character", () => {
  expect(run(indentTab, "ab|", 4, true)).toBe("ab  |");
  expect(run(indentTab, "ab|", 3, true)).toBe("ab |");
  expect(run(indentTab, "|x", 2, false)).toBe("\t|x");
});

test("Tab over a selection indents its lines; Shift+Tab takes one level back", () => {
  expect(run(indentTab, "|a\nb|", 2, true).replace("|", "")).toBe("  a\n  b");
  expect(run(indentTab, "|a\nb|", 8, false).replace("|", "")).toBe("\ta\n\tb");
  expect(run(dedentLines, "      a|", 4, true)).toBe("  a|");
  expect(run(dedentLines, "\t\ta|", 4, false)).toBe("\ta|");
});

test("Enter keeps the line's indentation as written", () => {
  expect(run(newlineKeepIndent, "\t  a|", 4, true)).toBe("\t  a\n\t  |");
  expect(run(newlineKeepIndent, "a|", 4, true)).toBe("a\n|");
});
