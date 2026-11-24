import { describe, expect, it } from "vitest";
import { changeStarts, fileName, fileStats, parsePatch, patchRows } from "./patch";

describe("parsePatch", () => {
  it("numbers lines on both sides of a git diff", () => {
    const files = parsePatch(
      [
        "diff --git a/src/a.ts b/src/a.ts",
        "index 1111111..2222222 100644",
        "--- a/src/a.ts",
        "+++ b/src/a.ts",
        "@@ -2,3 +2,3 @@ fn main",
        " keep",
        "-old",
        "+new",
        " tail",
        "\\ No newline at end of file",
      ].join("\n"),
    );
    expect(files).toEqual([
      {
        oldPath: "src/a.ts",
        newPath: "src/a.ts",
        hunks: [
          {
            header: "@@ -2,3 +2,3 @@ fn main",
            lines: [
              { kind: "context", text: "keep", old: 2, new: 2 },
              { kind: "removed", text: "old", old: 3 },
              { kind: "added", text: "new", new: 3 },
              { kind: "context", text: "tail", old: 4, new: 4 },
              { kind: "note", text: "\\ No newline at end of file" },
            ],
          },
        ],
      },
    ]);
  });

  it("uses hunk sizes to split files and keeps removed '-- ' lines", () => {
    const files = parsePatch(
      [
        "--- a/x.sql",
        "+++ b/x.sql",
        "@@ -1 +0,0 @@",
        "--- comment",
        "--- /dev/null",
        "+++ b/new.txt",
        "@@ -0,0 +1 @@",
        "+hi",
      ].join("\n"),
    );
    expect(files.map((f) => [f.oldPath, f.newPath])).toEqual([
      ["x.sql", "x.sql"],
      [null, "new.txt"],
    ]);
    expect(files[0]?.hunks[0]?.lines).toEqual([{ kind: "removed", text: "-- comment", old: 1 }]);
    expect(files[1]?.hunks[0]?.lines).toEqual([{ kind: "added", text: "hi", new: 1 }]);
  });

  it("returns nothing for text that is not a diff", () => {
    expect(parsePatch("just some notes\n+ not a hunk")).toEqual([]);
  });
});

describe("patch rows", () => {
  const files = parsePatch(
    [
      "--- a/a.txt",
      "+++ b/a.txt",
      "@@ -1,2 +1,2 @@",
      "-x",
      "+y",
      " z",
      "--- a/gone.txt",
      "+++ /dev/null",
      "@@ -1 +0,0 @@",
      "-q",
    ].join("\n"),
  );

  it("counts changes and names files", () => {
    expect(files.map(fileStats)).toEqual([
      { added: 1, removed: 1 },
      { added: 0, removed: 1 },
    ]);
    expect(files.map(fileName)).toEqual(["a.txt", "gone.txt"]);
  });

  it("finds where each run of changed lines starts", () => {
    expect(changeStarts(patchRows(files, new Set()))).toEqual([2, 7]);
    expect(changeStarts(patchRows(files, new Set([0])))).toEqual([3]);
  });

  it("flattens files and hunks, keeping only the header of collapsed files", () => {
    expect(patchRows(files, new Set()).map((row) => row.kind)).toEqual([
      "file",
      "hunk",
      "removed",
      "added",
      "context",
      "file",
      "hunk",
      "removed",
    ]);
    expect(patchRows(files, new Set([0]))).toEqual([
      { kind: "file", file: 0 },
      { kind: "file", file: 1 },
      { kind: "hunk", text: "@@ -1 +0,0 @@" },
      { kind: "removed", text: "q", old: 1 },
    ]);
  });
});
