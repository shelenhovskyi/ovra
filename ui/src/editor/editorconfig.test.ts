import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeAll, expect, test } from "vitest";
import { Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { configuredIndent, matches, parse, resolve, saveEdits } from "./editorconfig";
import { FakeCore, MemoryStorage } from "./fake-core";
import { indentLabel } from "./indent";

test("section globs match as EditorConfig says", () => {
  expect(matches("*", "a.ts")).toBe(true);
  expect(matches("*.ts", "src/deep/a.ts")).toBe(true);
  expect(matches("*.{js,ts}", "src/a.js")).toBe(true);
  expect(matches("*.{js,ts}", "src/a.rs")).toBe(false);
  expect(matches("src/*.ts", "src/a.ts")).toBe(true);
  expect(matches("src/*.ts", "src/deep/a.ts")).toBe(false);
  expect(matches("/src/**.ts", "src/deep/a.ts")).toBe(true);
  expect(matches("lib/**/a.ts", "lib/a.ts")).toBe(false);
  expect(matches("Makefile", "sub/Makefile")).toBe(true);
  expect(matches("file?.txt", "file1.txt")).toBe(true);
  expect(matches("[!a]b.txt", "cb.txt")).toBe(true);
  expect(matches("[!a]b.txt", "ab.txt")).toBe(false);
  expect(matches("v{1..3}.txt", "v2.txt")).toBe(true);
  expect(matches("v{1..3}.txt", "v4.txt")).toBe(false);
  expect(matches("a.b", "aXb")).toBe(false);
});

test("later sections and nearer files win; root stops nothing below it", () => {
  const top = parse(
    "root = true\n# comment\n[*]\nindent_style = space\nindent_size = 4\n\n[*.go]\nindent_style = tab\n",
  );
  const near = parse("[*.ts]\nINDENT_SIZE = 2\ntrim_trailing_whitespace = true\n");
  expect(top.root).toBe(true);
  expect(resolve([{ dir: "", parsed: top }], "x.go")).toEqual({ indentStyle: "tab", indentSize: 4 });
  expect(
    resolve(
      [
        { dir: "", parsed: top },
        { dir: "ui", parsed: near },
      ],
      "ui/a.ts",
    ),
  ).toEqual({ indentStyle: "space", indentSize: 2, trimTrailingWhitespace: true });
  // Out-of-range or unknown values are ignored.
  expect(resolve([{ dir: "", parsed: parse("[*]\nindent_size = 99\nindent_style = mixed\n") }], "a")).toEqual({});
});

test("the configured indentation wins over the detected one", () => {
  expect(configuredIndent({ indentStyle: "tab", tabWidth: 8 }, "Spaces: 2")).toEqual({ label: "Tab", tabWidth: 8 });
  expect(configuredIndent({ indentStyle: "space", indentSize: 3 }, "Tab")).toEqual({ label: "Spaces: 3", tabWidth: 3 });
  expect(configuredIndent({ indentSize: 2 }, null)).toEqual({ label: "Spaces: 2", tabWidth: 2 });
  expect(configuredIndent({ indentSize: 2 }, "Tab")).toEqual({ label: "Tab", tabWidth: 2 });
  expect(configuredIndent({}, "Spaces: 4")).toEqual({ label: "Spaces: 4", tabWidth: null });
});

test("saving trims line ends, except lines in `keep`, and adds the last line break", () => {
  const doc = Text.of(["a  ", "b\t", "c"]);
  const apply = (keep?: Set<number>) =>
    EditorState.create({ doc })
      .update({ changes: saveEdits({ trimTrailingWhitespace: true, insertFinalNewline: true }, doc, keep) })
      .state.doc.toString();
  expect(apply()).toBe("a\nb\nc\n");
  expect(apply(new Set([2]))).toBe("a\nb\t\nc\n");
  expect(saveEdits({}, doc)).toEqual([]);
});

beforeAll(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});
const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

test("a buffer indents and saves as the project's .editorconfig files say, and rereads them after a change", async () => {
  const core = new FakeCore();
  core.put(".editorconfig", "root = true\n[*]\nindent_style = tab\ntab_width = 8\n");
  core.put("src/.editorconfig", "[*.ts]\nindent_style = space\nindent_size = 2\ninsert_final_newline = true\n");
  core.put("src/a.ts", "let a;\n    b;");
  core.put("b.txt", "x");
  const buffers = new Buffers({
    transport: core,
    project: "p1",
    drafts: new Drafts(new MemoryStorage()),
    highlighter: () => null,
  });
  const view = new EditorView({ parent: document.body.appendChild(document.createElement("div")) });
  views.push(view);
  const a = buffers.open("src/a.ts");
  await a.loaded;
  a.attach(view);
  expect(indentLabel(view.state)).toBe("Spaces: 2");
  view.dispatch({ changes: { from: 0, insert: "x" } });
  await a.save();
  expect(core.writes.at(-1)?.text).toBe("xlet a;\n    b;\n");
  // The edit stays in the editor, so the text matches the file.
  expect([a.text, a.dirty]).toEqual(["xlet a;\n    b;\n", false]);

  const b = buffers.open("b.txt");
  await b.loaded;
  b.attach(view);
  expect([indentLabel(view.state), view.state.tabSize]).toEqual(["Tab", 8]);

  core.put("c.txt", "y");
  core.put(".editorconfig", "[*]\nindent_style = space\nindent_size = 3\ntrim_trailing_whitespace = true\n");
  core.changed(".editorconfig");
  const c = buffers.open("c.txt");
  await c.loaded;
  c.attach(view);
  expect(indentLabel(view.state)).toBe("Spaces: 3");
  // An automatic save leaves the line being typed alone.
  view.dispatch({ changes: { from: 1, insert: "  \nz  " }, selection: { anchor: 4 } });
  await c.save(false, true);
  expect(core.writes.at(-1)?.text).toBe("y\nz  ");
});
