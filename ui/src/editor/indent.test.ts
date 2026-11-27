import { EditorSelection, EditorState, type StateCommand } from "@codemirror/state";
import { expect, test } from "vitest";
import { dedentLines, indentation, indentTab, newlineKeepIndent, smartIndent } from "./indent";

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

/** Like `run`, with the language's block rules. */
function runIn(languageId: string | null, command: StateCommand, doc: string): string {
  const at = doc.indexOf("|");
  let state = EditorState.create({
    doc: doc.replace("|", ""),
    selection: EditorSelection.cursor(at),
    extensions: [indentation(2, true), smartIndent(languageId)],
  });
  command({ state, dispatch: (tr) => (state = tr.state) });
  const head = state.selection.main.head;
  const text = state.doc.toString();
  return `${text.slice(0, head)}|${text.slice(head)}`;
}

test("Enter after an opening bracket goes one level deeper; between a pair the closer gets its own line", () => {
  expect(runIn(null, newlineKeepIndent, "  if (a) {|")).toBe("  if (a) {\n    |");
  expect(runIn(null, newlineKeepIndent, "  f({|})")).toBe("  f({\n    |\n  })");
  expect(runIn(null, newlineKeepIndent, "x = [|  ]")).toBe("x = [\n  |\n]");
  // A closer of another kind stays where it is.
  expect(runIn(null, newlineKeepIndent, "f(|]")).toBe("f(\n  |]");
  expect(runIn(null, newlineKeepIndent, "a: |b")).toBe("a:\n|b");
});

test("Python goes deeper after a colon", () => {
  expect(runIn("python", newlineKeepIndent, "    def f():|")).toBe("    def f():\n      |");
});

/** Types `char` at `|` as the keyboard does. */
function typeIn(doc: string, char: string): string {
  const at = doc.indexOf("|");
  const state = EditorState.create({
    doc: doc.replace("|", ""),
    selection: EditorSelection.cursor(at),
    extensions: [indentation(2, true), smartIndent(null)],
  });
  const next = state.update({
    changes: { from: at, insert: char },
    selection: EditorSelection.cursor(at + 1),
    userEvent: "input.type",
  }).state;
  const head = next.selection.main.head;
  const text = next.doc.toString();
  return `${text.slice(0, head)}|${text.slice(head)}`;
}

test("a closing bracket typed first on its line lines up with the line that opened its pair", () => {
  expect(typeIn("  if (a) {\n    b\n    |", "}")).toBe("  if (a) {\n    b\n  }|");
  expect(typeIn("x = [\n  1,\n|", "]")).toBe("x = [\n  1,\n]|");
  // After other text on the line, or without a pair, nothing moves.
  expect(typeIn("  f(\n    a|", ")")).toBe("  f(\n    a)|");
  expect(typeIn("    |", "}")).toBe("    }|");
});
