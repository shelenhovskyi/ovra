import { setDiagnostics } from "@codemirror/lint";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, it } from "vitest";
import { editorSuggestions, fitBytes, problemAt, selectionOf } from "./agent-context";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

function editor(doc: string, anchor: number, head = anchor): EditorView {
  view = new EditorView({ state: EditorState.create({ doc, selection: EditorSelection.single(anchor, head) }) });
  return view;
}

it("names the selected lines; a selection ending at a line start leaves that line out", () => {
  const doc = "one\ntwo\nthree\n";
  expect(selectionOf(editor(doc, 0, 8), "a.ts")).toEqual({
    kind: "selection",
    path: "a.ts",
    from_line: 1,
    to_line: 2,
    text: "one\ntwo\n",
  });
  expect(selectionOf(editor(doc, 5), "a.ts")).toBeNull();
});

it("offers the selection and the file's problems, and finds the problem at the cursor", () => {
  const target = editor("let a = 1;\nlet b = 2;\n", 0, 3);
  target.dispatch(
    setDiagnostics(target.state, [{ from: 15, to: 16, severity: "warning", message: "unused b", source: "ts" }]),
  );
  expect(editorSuggestions(target, "a.ts").map((s) => s.label)).toEqual(["Selection", "Problem: unused b"]);
  expect(problemAt(target, "a.ts", 15)).toEqual({
    kind: "problem",
    path: "a.ts",
    line: 2,
    severity: "warning",
    message: "unused b",
    source: "ts",
  });
  expect(problemAt(target, "a.ts", 2)).toBeNull();
});

it("cuts text to the bytes the core takes, never inside a character", () => {
  expect(fitBytes("ääää", 5)).toBe("ää");
  expect(fitBytes("abc", 5)).toBe("abc");
});
