import { EditorState, Text, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { expect, test } from "vitest";
import { flattenSymbols, outlineSymbols, symbolSource } from "./symbols";

/** Enough of a view for the list: no language server, a state that takes the jump. */
let view: { state: EditorState };
function fakeView(doc: string): EditorView {
  view = {
    state: EditorState.create({ doc }),
    plugin: () => null,
    focus: () => {},
    dispatch(spec: TransactionSpec) {
      view.state = view.state.update(spec).state;
    },
  } as unknown as EditorView;
  return view as EditorView;
}

const at = (line: number, character: number) => ({ line, character });
const range = (line: number, from: number, to: number) => ({ start: at(line, from), end: at(line, to) });
const names = (symbols: { name: string; kind: string; container?: string }[]) =>
  symbols.map((s) => [s.name, s.kind, s.container ?? ""].join(" ").trim());

test("server symbols flatten in text order, nested ones naming their container", () => {
  const offset = ({ line, character }: { line: number; character: number }) => line * 100 + character;
  const nested = [
    {
      name: "Buffer",
      kind: 5,
      range: range(2, 0, 9),
      selectionRange: range(2, 6, 12),
      children: [{ name: "save", kind: 6, range: range(4, 2, 8), selectionRange: range(4, 2, 6) }],
    },
    { name: "open", kind: 12, range: range(0, 0, 3), selectionRange: range(0, 9, 13) },
  ];
  const symbols = flattenSymbols(nested, offset);
  expect(names(symbols)).toEqual(["open function", "Buffer class", "save method Buffer"]);
  expect([symbols[1]?.from, symbols[1]?.to]).toEqual([206, 212]);
  const flat = [{ name: "x", kind: 99, location: { uri: "file:///a", range: range(1, 4, 5) }, containerName: "m" }];
  expect(names(flattenSymbols(flat, offset))).toEqual(["x symbol m"]);
  expect(flattenSymbols(null, offset)).toEqual([]);
});

test("without a server, an outline finds functions, classes, types and Markdown headings", () => {
  const code = Text.of([
    "export async function load() {}",
    "class Store {",
    "  def save(self):",
    "export interface Options {}",
    "pub(crate) fn run() {}",
    "pub struct Pod;",
    "impl<T> Drop for Pod<T> {",
    "const x = 1;",
  ]);
  const symbols = outlineSymbols(code, "src/a.ts");
  expect(names(symbols)).toEqual([
    "load function",
    "Store class",
    "save function",
    "Options interface",
    "run function",
    "Pod struct",
    "Drop for Pod<T> impl",
  ]);
  expect(code.sliceString(symbols[4]?.from ?? 0, symbols[4]?.to)).toBe("run");
  const markdown = Text.of(["# Title", "```", "# not a heading", "```", "## Usage ##"]);
  expect(names(outlineSymbols(markdown, "README.md"))).toEqual(["Title heading 1", "Usage heading 2"]);
});

test("the palette list keeps text order when empty and Enter selects the symbol's name", async () => {
  const list = await symbolSource(fakeView("fn a() {}\nfn b() {}"), "src/lib.rs");
  const entries = await list.entries();
  expect(list.idle?.(entries).map((e) => [e.label, e.detail])).toEqual([
    ["a", "function"],
    ["b", "function"],
  ]);
  await entries[1]?.run();
  const { state } = view;
  expect(state.sliceDoc(state.selection.main.from, state.selection.main.to)).toBe("b");
  const empty = await symbolSource(fakeView("plain"), "notes.txt");
  expect([await empty.entries(), empty.empty]).toEqual([[], "No symbols found in notes.txt"]);
});
