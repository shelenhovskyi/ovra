import { setDiagnostics } from "@codemirror/lint";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, it } from "vitest";
import { agentAsks } from "../agents/context.svelte";
import { askAbout, editorSuggestions, fitBytes, problemAt, selectionOf, withAsk } from "./agent-context";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
  agentAsks.waiting = [];
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

it("asks about the problem a selection covers exactly, as after F8", () => {
  const target = editor("let a = 1;\nlet b = 2;\n", 15, 16);
  target.dispatch(setDiagnostics(target.state, [{ from: 15, to: 16, severity: "error", message: "bad b" }]));
  askAbout(target, "a.ts");
  expect(agentAsks.waiting.map((what) => what.kind)).toEqual(["problem"]);
});

it("puts Ask agent on the hover card's own diagnostics, once", () => {
  const target = editor("let a = 1;\n", 0);
  const diagnostic = { from: 4, to: 5, severity: "warning" as const, message: "unused a" };
  const shown = withAsk(
    withAsk([diagnostic], () => "a.ts"),
    () => "a.ts",
  );
  // The card finds an action's diagnostic by identity: a copy would do nothing.
  expect(shown[0]).toBe(diagnostic);
  expect(shown[0]?.actions?.map((a) => a.name)).toEqual(["Ask agent"]);
  shown[0]?.actions?.[0]?.apply(target, 4, 5);
  expect(agentAsks.waiting).toEqual([
    { kind: "problem", path: "a.ts", line: 1, severity: "warning", message: "unused a", source: null },
  ]);
});
