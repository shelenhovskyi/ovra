import type { ILink } from "@xterm/xterm";
import { describe, expect, it } from "vitest";
import type { FileLink } from "./file-links";
import { type BufferLike, fileLinkProvider, fileSet, outputLine } from "./link-provider";

/** A buffer of rows; a character in `wide` takes two cells, as xterm stores it. */
function buffer(rows: { text: string; wrapped?: boolean }[], wide = "\u{1F600}"): BufferLike {
  return {
    getLine: (y) => {
      const row = rows[y];
      if (!row) return undefined;
      const cells = [...row.text].flatMap((chars) =>
        wide.includes(chars)
          ? [
              { chars, width: 2 },
              { chars: "", width: 0 },
            ]
          : [{ chars, width: 1 }],
      );
      return {
        isWrapped: row.wrapped ?? false,
        length: cells.length,
        getCell: (x) => {
          const cell = cells[x];
          return cell && { getChars: () => cell.chars, getWidth: () => cell.width };
        },
      };
    },
  };
}

function provider(rows: { text: string; wrapped?: boolean }[], files: string[], opened: FileLink[] = []) {
  const hints: (string | null)[] = [];
  const links = fileLinkProvider({
    buffer: () => buffer(rows),
    root: () => "/w/app",
    base: "",
    files: async () => fileSet(files),
    opens: (event) => event.ctrlKey,
    open: (link) => opened.push(link),
    hint: (text) => hints.push(text),
    hintText: "Open file (Ctrl+click)",
  });
  const on = (row: number) =>
    new Promise<ILink[] | undefined>((resolve) => links.provideLinks(row, (found) => resolve(found)));
  return { on, hints, opened };
}

describe("outputLine", () => {
  it("joins wrapped rows and maps each character to its cells", () => {
    const rows = [{ text: "x" }, { text: "ab\u{1F600}" }, { text: "cd", wrapped: true }];
    const line = outputLine(buffer(rows), 2);
    expect(line.text).toBe("ab\u{1F600}cd");
    expect(line.at.map((p) => [p.x, p.y])).toEqual([
      [1, 2],
      [2, 2],
      [3, 2],
      [3, 2],
      [1, 3],
      [2, 3],
    ]);
    expect(line.to[2]).toEqual({ x: 4, y: 2 });
  });
});

describe("fileLinkProvider", () => {
  it("links only files the project has, over their cells", async () => {
    const { on } = provider([{ text: "\u{1F600} src/a.ts:3 src/gone.ts" }], ["src/a.ts"]);
    const links = await on(1);
    expect(links?.map((l) => [l.text, l.range])).toEqual([
      ["src/a.ts:3", { start: { x: 4, y: 1 }, end: { x: 13, y: 1 } }],
    ]);
  });

  it("finds a path that wraps onto the next row from either row", async () => {
    const rows = [{ text: "see src/" }, { text: "a.ts:2", wrapped: true }];
    const { on } = provider(rows, ["src/a.ts"]);
    const range = { start: { x: 5, y: 1 }, end: { x: 6, y: 2 } };
    expect((await on(1))?.map((l) => l.range)).toEqual([range]);
    expect((await on(2))?.map((l) => l.range)).toEqual([range]);
  });

  it("opens on Ctrl+click only, and hints while hovered", async () => {
    const { on, hints, opened } = provider([{ text: "ui/a.ts(4,2)" }], ["ui/a.ts"]);
    const [link] = (await on(1)) ?? [];
    if (!link) throw new Error("no link");
    link.activate(new MouseEvent("click"), link.text);
    expect(opened).toEqual([]);
    link.activate(new MouseEvent("click", { ctrlKey: true }), link.text);
    expect(opened).toMatchObject([{ path: "ui/a.ts", line: 4, column: 2 }]);
    link.hover?.(new MouseEvent("mouseover"), link.text);
    link.leave?.(new MouseEvent("mouseout"), link.text);
    expect(hints).toEqual(["Open file (Ctrl+click)", null]);
  });

  it("gives no links without candidates or when the file list fails", async () => {
    expect(await provider([{ text: "just words" }], []).on(1)).toBeUndefined();
    const failing = fileLinkProvider({
      buffer: () => buffer([{ text: "a.ts" }]),
      root: () => null,
      base: "",
      files: () => Promise.reject(new Error("down")),
      opens: () => true,
      open: () => {},
      hint: () => {},
      hintText: "",
    });
    expect(await new Promise((resolve) => failing.provideLinks(1, resolve))).toBeUndefined();
  });
});
