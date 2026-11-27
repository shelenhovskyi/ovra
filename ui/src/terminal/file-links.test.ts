import { describe, expect, it } from "vitest";
import { findFileLinks, linkSpot } from "./file-links";

const root = "/home/p/app";
/** The links in `text` as `[matched text, path, line, column]`. */
const found = (text: string, base = "", at: string | null = root) =>
  findFileLinks(text, at, base).map((l) => [text.slice(l.start, l.end), l.path, l.line, l.column]);

describe("findFileLinks", () => {
  it("finds paths with line and column suffixes, the whole suffix part of the link", () => {
    expect(found("src/main.rs:42")).toEqual([["src/main.rs:42", "src/main.rs", 42, null]]);
    expect(found("  --> src/main.rs:42:7")).toEqual([["src/main.rs:42:7", "src/main.rs", 42, 7]]);
    expect(found("./ui/a.ts(12,3): error TS2322")).toEqual([["./ui/a.ts(12,3)", "ui/a.ts", 12, 3]]);
    expect(found("ui/a.ts(12, 3)")).toEqual([["ui/a.ts(12, 3)", "ui/a.ts", 12, 3]]);
    expect(found("ui/a.ts(12)")).toEqual([["ui/a.ts(12)", "ui/a.ts", 12, null]]);
    expect(found("ui/a.ts")).toEqual([["ui/a.ts", "ui/a.ts", null, null]]);
    expect(found("Makefile:3")).toEqual([["Makefile:3", "Makefile", 3, null]]);
  });

  it("finds several paths on a line, inside quotes and brackets", () => {
    expect(found('M  "src/ab.rs" and (lib/x.ts:1:2), [c.md]')).toEqual([
      ["src/ab.rs", "src/ab.rs", null, null],
      ["lib/x.ts:1:2", "lib/x.ts", 1, 2],
      ["c.md", "c.md", null, null],
    ]);
    expect(found("    at run (/home/p/app/src/x.js:10:5)")).toEqual([["/home/p/app/src/x.js:10:5", "src/x.js", 10, 5]]);
  });

  it("takes absolute paths only under the project root", () => {
    expect(found("/home/p/app/src/lib.rs")).toEqual([["/home/p/app/src/lib.rs", "src/lib.rs", null, null]]);
    expect(found("/etc/passwd:1")).toEqual([]);
    expect(found("/home/p/apple/x.rs")).toEqual([]);
    expect(found("/home/p/app/../secret.txt")).toEqual([]);
    expect(found("C:\\Windows\\win.ini")).toEqual([]);
    expect(found("/home/p/app/x.rs", "", null)).toEqual([]);
  });

  it("takes the root's own spelling, with spaces, non-Latin letters and a trailing slash", () => {
    const spaced = "/home/p/My Proj \u00e9/";
    expect(found("/home/p/My Proj \u00e9/a/x.rs:3 done", "", spaced)).toEqual([
      ["/home/p/My Proj \u00e9/a/x.rs:3", "a/x.rs", 3, null],
    ]);
    expect(found("C:\\w\\app\\src\\a.ts(4,2)", "", "C:\\w\\app")).toEqual([
      ["C:\\w\\app\\src\\a.ts(4,2)", "src/a.ts", 4, 2],
    ]);
  });

  it("resolves relative paths from the shell's starting folder and keeps them in the project", () => {
    expect(found("src/a.ts", "ui")).toEqual([["src/a.ts", "ui/src/a.ts", null, null]]);
    expect(found("../README.md", "ui")).toEqual([["../README.md", "README.md", null, null]]);
    expect(found("../outside.txt")).toEqual([]);
    expect(found("src\\win\\a.rs:2")).toEqual([["src\\win\\a.rs:2", "src/win/a.rs", 2, null]]);
    expect(found("\u03b1\u03b2/\u03b3\u03b4.txt")).toEqual([
      ["\u03b1\u03b2/\u03b3\u03b4.txt", "\u03b1\u03b2/\u03b3\u03b4.txt", null, null],
    ]);
  });

  it("leaves URLs, plain words and a sentence's full stop alone", () => {
    expect(found("see https://example.com/src/main.rs:12 and http://localhost:3000/a.ts")).toEqual([]);
    expect(found("error: build failed")).toEqual([]);
    expect(found("Changed src/a.ts.")).toEqual([["src/a.ts", "src/a.ts", null, null]]);
    expect(found("a.rs:0")).toEqual([]);
  });

  it("counts offsets in UTF-16 units", () => {
    const text = "\u{1F600} src/a.ts:1";
    expect(findFileLinks(text, root)[0]).toMatchObject({ start: 3, end: text.length });
  });
});

describe("linkSpot", () => {
  it("puts the cursor on the line and column, from 0 in the line", () => {
    const [withColumn, lineOnly, bare] = findFileLinks("a.ts:4:7 b.ts:2 c.ts", root);
    expect(withColumn && linkSpot(withColumn)).toEqual({ line: 4, from: 6, to: 6 });
    expect(lineOnly && linkSpot(lineOnly)).toEqual({ line: 2, from: 0, to: 0 });
    expect(bare && linkSpot(bare)).toBeUndefined();
  });
});
