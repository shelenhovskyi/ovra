import { Text } from "@codemirror/state";
import { expect, test } from "vitest";
import { pathCrumbs, scopedOutline, symbolTrail } from "./breadcrumbs";
import type { FileSymbol } from "./symbols";

test("a path becomes its segments, each with the path up to it", () => {
  expect(pathCrumbs("src/editor/Editor.svelte")).toEqual([
    { name: "src", path: "src" },
    { name: "editor", path: "src/editor" },
    { name: "Editor.svelte", path: "src/editor/Editor.svelte" },
  ]);
  expect(pathCrumbs("README.md")).toEqual([{ name: "README.md", path: "README.md" }]);
});

test("the trail is the symbols around the cursor, outermost first", () => {
  const symbol = (name: string, from: number, to: number): FileSymbol => ({
    name,
    kind: "function",
    from,
    to: from + 1,
    scope: { from, to },
  });
  const symbols = [symbol("Buffer", 0, 100), symbol("save", 10, 40), symbol("load", 50, 90), symbol("x", 200, 300)];
  const names = (pos: number) => symbolTrail(symbols, pos).map((s) => s.name);
  expect(names(20)).toEqual(["Buffer", "save"]);
  expect(names(45)).toEqual(["Buffer"]);
  expect(names(150)).toEqual([]);
  // A symbol without a scope is never around the cursor.
  expect(symbolTrail([{ name: "y", kind: "", from: 0, to: 1 }], 0)).toEqual([]);
});

test("without a server, an outline symbol spans what its line folds", () => {
  const code = Text.of(["class A {", "  fn b() {", "    c();", "  }", "}", "fn d() {}"]);
  const trail = (pos: number) => symbolTrail(scopedOutline(code, "a.rs", 4), pos).map((s) => s.name);
  expect(trail(code.line(3).from + 2)).toEqual(["A", "b"]);
  expect(trail(code.line(6).from + 5)).toEqual(["d"]);
  const markdown = Text.of(["# Intro", "text", "## Use", "more", "# End"]);
  const headings = (pos: number) => symbolTrail(scopedOutline(markdown, "a.md", 4), pos).map((s) => s.name);
  expect(headings(markdown.line(4).from)).toEqual(["Intro", "Use"]);
  expect(headings(markdown.line(5).from)).toEqual(["End"]);
});
