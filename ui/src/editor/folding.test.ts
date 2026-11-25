import { Text } from "@codemirror/state";
import { expect, test } from "vitest";
import { codeFoldAt, type FoldRange, headingFoldAt } from "./folding";

/** The text a fold hides, or null. */
function hidden(lines: string[], fold: (doc: Text) => FoldRange | null): string | null {
  const doc = Text.of(lines);
  const range = fold(doc);
  return range && doc.sliceString(range.from, range.to);
}

test("a line folds the lines indented deeper below it, keeping the closing line in sight", () => {
  const lines = ["fn main() {", "    let a = 1;", "", "    if a {", "\tb();", "    }", "}", "x"];
  expect(hidden(lines, (doc) => codeFoldAt(doc, 1, 4))).toBe("\n    let a = 1;\n\n    if a {\n\tb();\n    }");
  // A tab is as deep as the tab size.
  expect(hidden(lines, (doc) => codeFoldAt(doc, 4, 4))).toBe("\n\tb();");
  expect(hidden(lines, (doc) => codeFoldAt(doc, 2, 4))).toBeNull();
  expect(hidden(lines, (doc) => codeFoldAt(doc, 3, 4))).toBeNull();
  expect(hidden(["def f():", "    pass"], (doc) => codeFoldAt(doc, 1, 4))).toBe("\n    pass");
});

test("without deeper lines, a line ending with a bracket folds to the bracket that closes it", () => {
  const lines = ["call({", "a: [1, 2],", "b: {}", "});"];
  expect(hidden(lines, (doc) => codeFoldAt(doc, 1, 4))).toBe("\na: [1, 2],\nb: {}");
  expect(hidden(["x = [", "1, 2];"], (doc) => codeFoldAt(doc, 1, 4))).toBe("\n1, 2");
  expect(hidden(["x = [", "1, 2"], (doc) => codeFoldAt(doc, 1, 4))).toBeNull();
  expect(hidden(["x = {}", "y"], (doc) => codeFoldAt(doc, 1, 4))).toBeNull();
});

test("a Markdown heading folds its section, up to the next heading at its level or above", () => {
  const lines = ["# A", "text", "## B", "```", "# not a heading", "```", "", "# C", "end"];
  expect(hidden(lines, (doc) => headingFoldAt(doc, 3))).toBe("\n```\n# not a heading\n```");
  expect(hidden(lines, (doc) => headingFoldAt(doc, 1))).toBe("\ntext\n## B\n```\n# not a heading\n```");
  expect(hidden(lines, (doc) => headingFoldAt(doc, 8))).toBe("\nend");
  expect(hidden(lines, (doc) => headingFoldAt(doc, 2))).toBeNull();
  expect(hidden(["# A", "", "# B"], (doc) => headingFoldAt(doc, 1))).toBeNull();
});
