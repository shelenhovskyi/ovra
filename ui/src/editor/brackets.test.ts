import { deleteBracketPair, insertBracket } from "@codemirror/autocomplete";
import { matchBrackets } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import { expect, test } from "vitest";
import { autoClose, bracketMarks, closingPairs } from "./brackets";
import { languageFor } from "./languages";

function state(doc: string, path: string): EditorState {
  const at = doc.indexOf("|");
  return EditorState.create({
    doc: doc.replace("|", ""),
    selection: EditorSelection.cursor(at),
    extensions: [bracketMarks(), autoClose(languageFor(path))],
  });
}

/** Types `char` the way closeBrackets' input handler does, returning the text with `|` at the cursor. */
function type(doc: string, char: string, path = "a.ts"): string {
  const before = state(doc, path);
  const after = insertBracket(before, char)?.state ?? before.update(before.replaceSelection(char)).state;
  const head = after.selection.main.head;
  const text = after.doc.toString();
  return `${text.slice(0, head)}|${text.slice(head)}`;
}

test("an opening bracket or quote adds its closing one before space or a closer, not before a word", () => {
  expect(type("f|", "(")).toBe("f(|)");
  expect(type("[|]", "{")).toBe("[{|}]");
  expect(type("x = |", '"')).toBe('x = "|"');
  expect(type("x = |", "`")).toBe("x = `|`");
  expect(type("|word", "(")).toBe("(|word");
});

test("typing the closing bracket that was added steps over it", () => {
  const opened = insertBracket(state("f|", "a.ts"), "(")?.state;
  const closed = opened && insertBracket(opened, ")")?.state;
  expect(closed?.doc.toString()).toBe("f()");
  expect(closed?.selection.main.head).toBe(3);
});

test("Rust keeps a single quote single, for lifetimes; plain text closes brackets and double quotes", () => {
  expect(type("&|", "'", "lib.rs")).toBe("&'|");
  expect(type("&|", "'", "a.py")).toBe("&'|'");
  expect(closingPairs(languageFor("notes.txt"))).toEqual(["(", "[", "{", '"']);
});

test("Backspace between an empty pair removes both", () => {
  const before = state("f(|)", "a.ts");
  let after = before;
  deleteBracketPair({ state: before, dispatch: (tr) => (after = tr.state) });
  expect(after.doc.toString()).toBe("f");
});

test("the bracket paired with the one at the cursor is found without a syntax tree", () => {
  const s = state("fn a() {\n  if (b) { c }\n|}", "lib.rs");
  const match = matchBrackets(s, s.selection.main.head + 1, -1);
  expect(match?.matched).toBe(true);
  expect(match?.end?.from).toBe(7);
});
