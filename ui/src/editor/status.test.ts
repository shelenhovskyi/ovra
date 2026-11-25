import { EditorSelection, EditorState } from "@codemirror/state";
import { describe, expect, test } from "vitest";
import {
  clearFacts,
  cursorLabel,
  cursorOf,
  detectIndent,
  fileStatus,
  lineEndingOf,
  publishFacts,
} from "./status.svelte";

describe("cursor", () => {
  test("says line and column, and the selection's size when there is one", () => {
    expect(cursorLabel({ line: 12, column: 5, selected: 0 })).toBe("Ln 12, Col 5");
    expect(cursorLabel({ line: 1, column: 1, selected: 3 })).toBe("Ln 1, Col 1 (3 selected)");
  });

  test("is 1-based and follows the selection's head", () => {
    const state = EditorState.create({ doc: "one\ntwo words", selection: EditorSelection.range(8, 4) });
    expect(cursorOf(state)).toEqual({ line: 2, column: 1, selected: 4 });
  });
});

describe("indentation", () => {
  test("finds the step between indented lines", () => {
    expect(detectIndent(["fn main() {", "    if x {", "        y();", "    }", "}"])).toBe("Spaces: 4");
    expect(detectIndent(["a:", "  b:", "    c: 1", "  d: 2"])).toBe("Spaces: 2");
  });

  test("says Tab when most indented lines start with one", () => {
    expect(detectIndent(["func main() {", "\tx := 1", "\tif x {", "\t\treturn", "\t}", "}"])).toBe("Tab");
  });

  test("skips block comment stars and blank lines", () => {
    expect(detectIndent(["/**", " * Docs.", " */", "class A {", "", "    m() {}", "}"])).toBe("Spaces: 4");
  });

  test("falls back to what the editor inserts", () => {
    expect(detectIndent(["no indentation", "at all"])).toBe("Spaces: 2");
    expect(detectIndent([])).toBe("Spaces: 2");
  });
});

test("line endings come from the first line break", () => {
  expect(lineEndingOf("a\r\nb\r\n")).toBe("CRLF");
  expect(lineEndingOf("a\nb\r\n")).toBe("LF");
  expect(lineEndingOf("one line")).toBe("LF");
});

test("only the publisher clears the facts, so a late clear keeps the next file's", () => {
  const editor = {};
  const viewer = {};
  const facts = { cursor: null, language: "Rust", indent: "Tab", lineEnding: "LF" } as const;
  publishFacts(viewer, facts);
  clearFacts(editor);
  expect(fileStatus.facts).toEqual(facts);
  clearFacts(viewer);
  expect(fileStatus.facts).toBeNull();
});
