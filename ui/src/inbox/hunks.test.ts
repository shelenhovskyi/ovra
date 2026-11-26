import { describe, expect, it } from "vitest";
import type { Hunk, LineComment } from "../lib/protocol";
import { commentLine, commentsAfterDiscard, hunkPlace, parseHunks } from "./hunks";

const PATCH = [
  "diff --git a/src/a.ts b/src/a.ts",
  "index 1..2 100644",
  "--- a/src/a.ts",
  "+++ b/src/a.ts",
  "@@ -1,3 +1,3 @@ function top()",
  "-one\r",
  "+uno\r",
  " two\r",
  " three\r",
  "@@ -10,2 +10,0 @@",
  "-gone",
  "-also gone",
  "\\ No newline at end of file",
  "",
].join("\n");

describe("parseHunks", () => {
  it("gives each hunk back as the core needs it, with numbered lines", () => {
    const [first, second] = parseHunks(PATCH);
    expect(first?.hunk).toEqual({
      old_start: 1,
      old_lines: 3,
      new_start: 1,
      new_lines: 3,
      header: "function top()",
      // A CRLF file's lines keep their `\r`, or the hunk would not match it.
      lines: ["-one\r", "+uno\r", " two\r", " three\r"],
    });
    expect(first?.lines.slice(0, 3)).toEqual([
      { kind: "removed", text: "one", old: 1 },
      { kind: "added", text: "uno", new: 1 },
      { kind: "context", text: "two", old: 2, new: 2 },
    ]);
    expect(second?.hunk.lines).toEqual(["-gone", "-also gone", "\\ No newline at end of file"]);
    expect(first && hunkPlace(first)).toBe("lines 1–3");
    expect(second && hunkPlace(second)).toBe("lines 10–11 of the old version");
    const [removed, added] = first?.lines ?? [];
    expect(removed && commentLine(removed)).toEqual({ line: 1, old: true });
    expect(added && commentLine(added)).toEqual({ line: 1, old: false });
    expect(parseHunks("Binary files differ\n")).toEqual([]);
  });

  it("reads an empty line as blank context, as diff.suppressBlankEmpty prints it", () => {
    const [hunk, next] = parseHunks(
      ["@@ -1,3 +1,3 @@", " a", "", "-b", "+c", "@@ -9 +9 @@", "-x", "+y", ""].join("\n"),
    );
    expect(hunk?.hunk.lines).toEqual([" a", " ", "-b", "+c"]);
    expect(hunk?.lines[1]).toEqual({ kind: "context", text: "", old: 2, new: 2 });
    expect(next?.hunk.lines).toEqual(["-x", "+y"]);
    // Past the header's counts an empty line ends the hunk.
    expect(parseHunks(["@@ -1 +1 @@", "-a", "+b", "", "junk"].join("\n"))[0]?.hunk.lines).toEqual(["-a", "+b"]);
  });

  it("names identical hunks apart", () => {
    const patch = ["@@ -1 +1 @@", "-a", "+b", "@@ -5 +5 @@", "-a", "+b", "@@ -9 +9 @@", "-a", "+b"].join("\n");
    const keys = parseHunks(patch).map((h) => h.key);
    expect(new Set(keys).size).toBe(3);
    expect(keys[0]).toBe("-a\n+b");
  });
});

describe("commentsAfterDiscard", () => {
  const hunk = (old_start: number, old_lines: number, new_start: number, new_lines: number): Hunk => ({
    old_start,
    old_lines,
    new_start,
    new_lines,
    header: "",
    lines: [],
  });
  const at = (line: number, old = false, path = "a.ts"): LineComment => ({
    path,
    line,
    ...(old ? { old: true } : {}),
    text: `${old ? "old" : "new"} ${line}`,
  });

  it("drops comments inside the hunk and moves later lines of the agent's version", () => {
    // Old lines 10-11 became new lines 10-13: discarding takes two lines out.
    const comments = [at(3), at(10), at(13), at(14), at(11, true), at(20, true), at(14, false, "b.ts")];
    expect(commentsAfterDiscard(comments, "a.ts", hunk(10, 2, 10, 4))).toEqual([
      at(3),
      { ...at(14), line: 12 },
      at(20, true),
      at(14, false, "b.ts"),
    ]);
  });

  it("handles hunks that only add or only remove lines", () => {
    // Added new lines 5-6 after old line 4.
    expect(commentsAfterDiscard([at(4), at(5), at(7), at(5, true)], "a.ts", hunk(4, 0, 5, 2))).toEqual([
      at(4),
      { ...at(7), line: 5 },
      at(5, true),
    ]);
    // Removed old lines 5-6, after new line 4.
    expect(commentsAfterDiscard([at(4), at(5), at(5, true), at(7, true)], "a.ts", hunk(5, 2, 4, 0))).toEqual([
      at(4),
      { ...at(5), line: 7 },
      at(7, true),
    ]);
  });
});
